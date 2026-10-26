package dev.signet.link

import org.json.JSONObject
import java.io.IOException
import java.io.InputStream
import java.io.OutputStream
import java.net.InetAddress
import java.net.InetSocketAddress
import java.net.ServerSocket
import java.net.Socket
import java.util.concurrent.ConcurrentHashMap
import java.util.concurrent.CountDownLatch
import java.util.concurrent.ExecutorService
import java.util.concurrent.TimeUnit
import javax.net.ssl.SSLSocket

/** Where to send `localhost`: the computer's addresses and the link credentials. */
data class LocalhostTarget(val endpoints: List<Endpoint>, val linkId: String, val token: String)

/**
 * "Mirror" ports: the phone listens on its own localhost (127.0.0.1 and ::1), at the ports that listen on the computer,
 * and passes each connection as is through a pinned TLS tunnel to the same port on the computer. The WebView therefore opens
 * `http://localhost:5173` without a proxy (same principle as iOS, where a real device never sends localhost to a proxy).
 * A port already taken on the phone is ignored.
 */
class LocalhostRelay(
  private val io: ExecutorService,
  private val onTunnelError: (port: Int, code: String) -> Unit,
) {
  @Volatile private var target: LocalhostTarget? = null

  /** Listeners per port (one for 127.0.0.1, one for ::1 if possible). */
  private val listeners = HashMap<Int, List<ServerSocket>>()

  /** Last address that answered (host:port), tried first. */
  @Volatile private var preferred: String? = null

  /** Current WebView connections, to cut them on stop. */
  private val active = java.util.Collections.newSetFromMap(ConcurrentHashMap<Socket, Boolean>())

  /** Avoids flooding JS when all of a page's connections fail for the same reason. */
  private val lastErrors = ConcurrentHashMap<String, Long>()

  /** Sets the route and the relayed ports. */
  @Synchronized
  fun setRoute(newTarget: LocalhostTarget, ports: List<Int>) {
    if (target?.linkId != newTarget.linkId) preferred = null
    target = newTarget
    val wanted = ports.filter { it in 1..65535 }.toSet()
    for (port in listeners.keys.toList()) {
      if (port !in wanted) listeners.remove(port)?.forEach { closeQuietly(it) }
    }
    for (port in wanted.sorted()) if (port !in listeners) listen(port)
  }

  @Synchronized
  fun stop() {
    target = null
    preferred = null
    listeners.values.flatten().forEach { closeQuietly(it) }
    listeners.clear()
    active.toList().forEach { closeQuietly(it) }
    active.clear()
  }

  private fun listen(port: Int) {
    val sockets = ArrayList<ServerSocket>()
    for (host in listOf("127.0.0.1", "::1")) {
      try {
        val server = ServerSocket()
        server.bind(InetSocketAddress(InetAddress.getByName(host), port), 50)
        sockets.add(server)
        io.execute { acceptLoop(server, port) }
      } catch (_: IOException) {
        // port already taken on the phone (or no IPv6): ignored
      }
    }
    listeners[port] = sockets
  }

  private fun acceptLoop(server: ServerSocket, port: Int) {
    while (!server.isClosed) {
      val client = try {
        server.accept()
      } catch (_: IOException) {
        return
      }
      active.add(client)
      try {
        io.execute { handle(client, port) }
      } catch (_: Exception) {
        closeQuietly(client)
        active.remove(client)
      }
    }
  }

  private fun handle(client: Socket, port: Int) {
    var tunnel: SSLSocket? = null
    try {
      client.tcpNoDelay = true
      val clientIn = client.getInputStream()
      val clientOut = client.getOutputStream()
      val opened = try {
        openTunnel(port)
      } catch (e: LinkException) {
        reportError(port, e.code)
        // Almost always a web page: a readable HTTP response rather than a dropped connection.
        return respond(clientOut, 502, "Bad Gateway", "Signet could not reach localhost:$port on your computer (${e.code}).")
      }
      tunnel = opened.first
      pipe(client, clientIn, clientOut, opened.first, opened.second)
    } catch (_: IOException) {
      // the WebView gave up: nothing to do
    } finally {
      closeQuietly(tunnel)
      closeQuietly(client)
      active.remove(client)
    }
  }

  private fun reportError(port: Int, code: String) {
    val key = "$port/$code"
    val now = System.currentTimeMillis()
    val last = lastErrors[key]
    if (last != null && now - last < 2_000) return
    lastErrors[key] = now
    onTunnelError(port, code)
  }

  /**
   * Opens a tunnel to `port` on the computer: first address that answers (the last successful one first), `hello`, then `ok`.
   * Returns the socket and its input stream (which may already contain raw bytes after the `ok`).
   */
  private fun openTunnel(port: Int): Pair<SSLSocket, InputStream> {
    val current = target ?: throw LinkException("no-route")
    val first = preferred
    val ordered = current.endpoints.sortedBy { if ("${it.host}:${it.port}" == first) 0 else 1 }
    var failure: LinkException? = null
    for (endpoint in ordered) {
      val socket = try {
        PinnedTls.connect(endpoint, CONNECT_TIMEOUT_MS)
      } catch (e: LinkException) {
        // A rejected fingerprint says more than a timeout.
        if (failure == null || e.code == "pin-mismatch") failure = e
        continue
      }
      try {
        val input = bufferedInput(socket)
        val hello = JSONObject()
          .put("type", "hello").put("v", 1).put("linkId", current.linkId).put("token", current.token)
          .put("role", "tunnel").put("port", port)
        socket.outputStream.write((hello.toString() + "\n").toByteArray(Charsets.UTF_8))
        socket.outputStream.flush()
        socket.soTimeout = REPLY_TIMEOUT_MS
        val reply = JSONObject(readLine(input) ?: throw LinkException("closed"))
        socket.soTimeout = 0
        if (reply.optString("type") == "ok") {
          preferred = "${endpoint.host}:${endpoint.port}"
          return socket to input
        }
        // An error response does come from the computer: no point trying the other addresses.
        closeQuietly(socket)
        throw LinkException(reply.optString("code").ifEmpty { "io" })
      } catch (e: LinkException) {
        closeQuietly(socket)
        throw e
      } catch (e: java.net.SocketTimeoutException) {
        closeQuietly(socket)
        failure = LinkException("timeout", e)
      } catch (e: Exception) {
        closeQuietly(socket)
        failure = LinkException("io", e)
      }
    }
    throw failure ?: LinkException("unreachable")
  }

  /** Relays in both directions until one side closes. */
  private fun pipe(client: Socket, clientIn: InputStream, clientOut: OutputStream, tunnel: SSLSocket, tunnelIn: InputStream) {
    val clientDone = CountDownLatch(1)
    io.execute {
      // computer -> WebView
      try {
        copy(tunnelIn, clientOut)
      } catch (_: IOException) {
      }
      // Clean end towards the WebView, then give it a moment to close on its side before cutting everything.
      try {
        client.shutdownOutput()
      } catch (_: IOException) {
      }
      clientDone.await(CLOSE_GRACE_MS, TimeUnit.MILLISECONDS)
      closeQuietly(tunnel)
      closeQuietly(client)
    }
    // WebView -> computer
    try {
      copy(clientIn, tunnel.outputStream)
    } catch (_: IOException) {
    }
    closeQuietly(tunnel) // SSLSocket has no half-close: the response in progress is cut too
    clientDone.countDown()
  }

  private fun copy(input: InputStream, output: OutputStream) {
    val buffer = ByteArray(16 * 1024)
    while (true) {
      val n = input.read(buffer)
      if (n < 0) return
      output.write(buffer, 0, n)
      output.flush()
    }
  }

  private fun respond(out: OutputStream, status: Int, reason: String, text: String) {
    val body = text.toByteArray(Charsets.UTF_8)
    val head = "HTTP/1.1 $status $reason\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Length: ${body.size}\r\nConnection: close\r\n\r\n"
    try {
      out.write(head.toByteArray(Charsets.ISO_8859_1))
      out.write(body)
      out.flush()
    } catch (_: IOException) {
    }
  }

  companion object {
    private const val CONNECT_TIMEOUT_MS = 4_000
    private const val REPLY_TIMEOUT_MS = 10_000
    private const val CLOSE_GRACE_MS = 3_000L
  }
}

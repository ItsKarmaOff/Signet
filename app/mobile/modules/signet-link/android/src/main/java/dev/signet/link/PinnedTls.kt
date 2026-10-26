package dev.signet.link

import java.io.BufferedInputStream
import java.io.ByteArrayOutputStream
import java.io.IOException
import java.io.InputStream
import java.net.ConnectException
import java.net.InetAddress
import java.net.InetSocketAddress
import java.net.NoRouteToHostException
import java.net.Socket
import java.net.SocketTimeoutException
import java.net.UnknownHostException
import java.security.MessageDigest
import java.security.cert.CertificateException
import java.security.cert.X509Certificate
import javax.net.ssl.SNIHostName
import javax.net.ssl.SSLContext
import javax.net.ssl.SSLException
import javax.net.ssl.SSLSocket
import javax.net.ssl.X509TrustManager

/** An address of the computer and the expected certificate fingerprint. */
data class Endpoint(val host: String, val port: Int, val serverName: String, val certSha256: String)

/** Error carrying a code from the JS contract (SocketError or protocol code). */
class LinkException(val code: String, cause: Throwable? = null) : Exception(code, cause)

const val MAX_LINE_BYTES = 64 * 1024

/** Opens a TLS socket where only the leaf certificate matters (SHA-256 in base64url without `=`). */
object PinnedTls {
  /** Compares the leaf certificate fingerprint and skips any other check (no authority, no hostname). */
  private class PinTrustManager(private val expected: String) : X509TrustManager {
    @Volatile var mismatch = false

    override fun checkServerTrusted(chain: Array<X509Certificate>?, authType: String?) {
      val leaf = chain?.firstOrNull() ?: throw CertificateException("no certificate")
      val digest = MessageDigest.getInstance("SHA-256").digest(leaf.encoded)
      val actual = android.util.Base64.encodeToString(
        digest,
        android.util.Base64.URL_SAFE or android.util.Base64.NO_PADDING or android.util.Base64.NO_WRAP,
      )
      if (!MessageDigest.isEqual(actual.toByteArray(), expected.toByteArray())) {
        mismatch = true
        throw CertificateException("pin-mismatch")
      }
    }

    override fun checkClientTrusted(chain: Array<X509Certificate>?, authType: String?) {}
    override fun getAcceptedIssuers(): Array<X509Certificate> = emptyArray()
  }

  /** Blocking: call off the main thread. Throws LinkException (timeout, refused, unreachable, pin-mismatch, tls, io). */
  fun connect(endpoint: Endpoint, timeoutMs: Int): SSLSocket {
    val raw = Socket()
    try {
      val address = InetAddress.getByName(endpoint.host)
      raw.tcpNoDelay = true
      raw.connect(InetSocketAddress(address, endpoint.port), timeoutMs)

      val trust = PinTrustManager(endpoint.certSha256)
      val context = SSLContext.getInstance("TLS")
      context.init(null, arrayOf(trust), null)
      // The name passed to createSocket is used as SNI: never the IP address.
      val socket = context.socketFactory.createSocket(raw, endpoint.serverName, endpoint.port, true) as SSLSocket
      try {
        val params = socket.sslParameters
        params.serverNames = listOf(SNIHostName(endpoint.serverName))
        val wanted = socket.supportedProtocols.filter { it == "TLSv1.2" || it == "TLSv1.3" }
        if (wanted.isNotEmpty()) params.protocols = wanted.toTypedArray()
        socket.sslParameters = params
        socket.soTimeout = timeoutMs // also limits the handshake
        socket.startHandshake()
        socket.soTimeout = 0
        return socket
      } catch (e: Exception) {
        if (trust.mismatch) throw LinkException("pin-mismatch", e)
        throw e
      }
    } catch (e: LinkException) {
      closeQuietly(raw)
      throw e
    } catch (e: Exception) {
      closeQuietly(raw)
      throw LinkException(codeFor(e), e)
    }
  }

  private fun codeFor(e: Throwable): String {
    var cause: Throwable? = e
    while (cause != null) {
      if (cause is SocketTimeoutException) return "timeout"
      cause = cause.cause
    }
    return when {
      e is ConnectException -> if (e.message?.contains("refused", ignoreCase = true) == true) "refused" else "unreachable"
      e is NoRouteToHostException || e is UnknownHostException -> "unreachable"
      e is SSLException -> "tls"
      e is IllegalArgumentException -> "tls" // nom SNI invalide
      else -> "io"
    }
  }
}

fun closeQuietly(socket: Socket?) {
  try {
    socket?.close()
  } catch (_: IOException) {
  }
}

fun closeQuietly(server: java.net.ServerSocket?) {
  try {
    server?.close()
  } catch (_: IOException) {
  }
}

/**
 * Reads a `\n`-terminated line byte by byte (without reading further: what follows may be raw bytes).
 * Returns null if the stream ends before the first byte; throws line-too-long beyond 64 KiB.
 */
fun readLine(input: InputStream): String? {
  val buffer = ByteArrayOutputStream()
  while (true) {
    val b = input.read()
    if (b < 0) return if (buffer.size() == 0) null else buffer.toString("UTF-8")
    if (b == '\n'.code) return buffer.toString("UTF-8")
    if (buffer.size() >= MAX_LINE_BYTES) throw LinkException("line-too-long")
    buffer.write(b)
  }
}

fun bufferedInput(socket: Socket): BufferedInputStream = BufferedInputStream(socket.getInputStream())

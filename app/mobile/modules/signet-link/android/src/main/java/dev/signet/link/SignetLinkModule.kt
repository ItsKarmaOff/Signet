package dev.signet.link

import android.os.Build
import android.provider.Settings
import expo.modules.kotlin.Promise
import expo.modules.kotlin.modules.Module
import expo.modules.kotlin.modules.ModuleDefinition
import expo.modules.kotlin.records.Field
import expo.modules.kotlin.records.Record
import java.util.concurrent.ConcurrentHashMap
import java.util.concurrent.ExecutorService
import java.util.concurrent.Executors

class PinnedEndpointRecord : Record {
  @Field val host: String = ""
  @Field val port: Int = 0
  @Field val serverName: String = ""
  @Field val certSha256: String = ""

  fun toEndpoint() = Endpoint(host, port, serverName, certSha256)
}

class LocalhostRouteRecord : Record {
  @Field val endpoints: List<PinnedEndpointRecord> = emptyList()
  @Field val linkId: String = ""
  @Field val token: String = ""
  @Field val ports: List<Int> = emptyList()

  fun toTarget() = LocalhostTarget(endpoints.map { it.toEndpoint() }, linkId, token)
}

/** `SignetLink` module: pinned TLS sockets, mDNS discovery, localhost relay for the WebView. Contract: ../README.md. */
class SignetLinkModule : Module() {
  // Threads for connections and (blocking) reads, off the main thread.
  private val io: ExecutorService = Executors.newCachedThreadPool()
  private val sockets = ConcurrentHashMap<String, LineSocket>()

  private val context get() = requireNotNull(appContext.reactContext) { "Contexte React indisponible" }

  private val browser by lazy {
    ServiceBrowser(
      context,
      onFound = { s ->
        emit(
          "onServiceFound",
          mapOf("name" to s.name, "desktopId" to s.desktopId, "version" to s.version, "hosts" to ArrayList(s.hosts), "port" to s.port),
        )
      },
      onLost = { name -> emit("onServiceLost", mapOf("name" to name)) },
    )
  }

  private val relay by lazy {
    LocalhostRelay(io) { port, code -> emit("onTunnelError", mapOf("port" to port, "code" to code)) }
  }


  override fun definition() = ModuleDefinition {
    Name("SignetLink")

    Events("onSocketLine", "onSocketClosed", "onServiceFound", "onServiceLost", "onTunnelError")

    AsyncFunction("open") { id: String, endpoint: PinnedEndpointRecord, timeoutMs: Int, promise: Promise ->
      openSocket(id, endpoint.toEndpoint(), timeoutMs, promise)
    }

    AsyncFunction("writeLine") { id: String, line: String, promise: Promise ->
      val socket = sockets[id]
      if (socket == null) {
        promise.reject("closed", "Socket closed", null)
      } else {
        socket.writeLine(line) { error ->
          if (error == null) promise.resolve() else promise.reject(error, "Write failed", null)
        }
      }
    }

    Function("close") { id: String ->
      sockets[id]?.finish(null)
      Unit
    }

    Function("startBrowsing") {
      browser.start()
      Unit
    }

    Function("stopBrowsing") {
      browser.stop()
      Unit
    }

    // Mirror ports (LocalhostRelay): supported everywhere.
    Function("isLocalhostRoutingSupported") {
      true
    }

    AsyncFunction("setLocalhostRoute") { route: LocalhostRouteRecord?, promise: Promise ->
      setLocalhostRoute(route, promise)
    }

    // The mirror ports serve every WebView: nothing to attach.
    AsyncFunction("attachToWebView") { _: Int ->
      true
    }

    Function("deviceName") {
      deviceName()
    }

    OnDestroy {
      sockets.values.toList().forEach { it.finish(null) }
      browser.stop()
      relay.stop()
      io.shutdown()
    }
  }

  private fun emit(name: String, body: Map<String, Any?>) {
    try {
      sendEvent(name, body)
    } catch (_: Exception) {
      // module destroyed: nobody is listening
    }
  }

  // region Sockets

  private fun openSocket(id: String, endpoint: Endpoint, timeoutMs: Int, promise: Promise) {
    if (sockets.containsKey(id)) {
      promise.reject("io", "Identifier already in use", null)
      return
    }
    io.execute {
      val tls = try {
        PinnedTls.connect(endpoint, timeoutMs)
      } catch (e: LinkException) {
        promise.reject(e.code, "Connexion impossible (${e.code})", null)
        return@execute
      }
      val socket = LineSocket(
        id,
        tls,
        onLine = { line -> emit("onSocketLine", mapOf("id" to id, "line" to line)) },
        onClosed = { error ->
          sockets.remove(id)
          emit("onSocketClosed", if (error == null) mapOf("id" to id) else mapOf("id" to id, "error" to error))
        },
      )
      if (sockets.putIfAbsent(id, socket) != null) {
        closeQuietly(tls)
        promise.reject("io", "Identifier already in use", null)
        return@execute
      }
      promise.resolve()
      socket.readLoop() // this thread becomes the read thread
    }
  }

  // endregion

  // region Localhost relay

  private fun setLocalhostRoute(route: LocalhostRouteRecord?, promise: Promise) {
    if (route == null) relay.stop() else relay.setRoute(route.toTarget(), route.ports)
    promise.resolve(true)
  }

  // endregion

  private fun deviceName(): String {
    val name = try {
      Settings.Global.getString(context.contentResolver, "device_name")
    } catch (_: Exception) {
      null
    }
    if (!name.isNullOrBlank()) return name.trim()
    val manufacturer = Build.MANUFACTURER.orEmpty().trim()
    val model = Build.MODEL.orEmpty().trim()
    // Many models already repeat the manufacturer ("samsung SM-S918B" but "Google Pixel 8").
    val full = if (model.startsWith(manufacturer, ignoreCase = true)) model else "$manufacturer $model"
    return full.trim().replaceFirstChar { it.uppercase() }.ifEmpty { "Android" }
  }
}

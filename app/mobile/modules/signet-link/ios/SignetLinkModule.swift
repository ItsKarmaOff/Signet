import ExpoModulesCore
import Network
import UIKit

struct PinnedEndpointRecord: Record {
  @Field var host: String = ""
  @Field var port: Int = 0
  @Field var serverName: String = ""
  @Field var certSha256: String = ""

  var target: PinnedTarget { PinnedTarget(host: host, port: port, serverName: serverName, certSha256: certSha256) }
}

struct LocalhostRouteRecord: Record {
  @Field var endpoints: [PinnedEndpointRecord] = []
  @Field var linkId: String = ""
  @Field var token: String = ""
  @Field var ports: [Int] = []
}

/// Link with the computer: pinned TLS sockets, Bonjour discovery, localhost relay for the WebView.
/// Networking runs on the serial queue `queue`.
public class SignetLinkModule: Module {
  private let queue = DispatchQueue(label: "dev.signet.link", qos: .userInitiated)
  /// Sockets open (or being opened) by identifier; only on `queue`.
  private var sockets: [String: PinnedConnection] = [:]
  private lazy var browser = LinkBrowser(queue: queue)
  private lazy var relay = LocalhostRelay(queue: queue)
  private var foregroundObserver: NSObjectProtocol?

  public func definition() -> ModuleDefinition {
    Name("SignetLink")

    Events("onSocketLine", "onSocketClosed", "onServiceFound", "onServiceLost", "onTunnelError")

    OnCreate {
      self.browser.onFound = { [weak self] f in
        self?.sendEvent("onServiceFound", ["name": f.name, "desktopId": f.desktopId, "version": f.version, "hosts": f.hosts, "port": f.port])
      }
      self.browser.onLost = { [weak self] name in self?.sendEvent("onServiceLost", ["name": name]) }
      self.relay.onTunnelError = { [weak self] port, code in self?.sendEvent("onTunnelError", ["port": port, "code": code]) }
      // On return to the foreground, the mirror ports are restarted (see LocalhostRelay.restartIfActive).
      self.foregroundObserver = NotificationCenter.default.addObserver(
        forName: UIApplication.willEnterForegroundNotification, object: nil, queue: nil
      ) { [weak self] _ in
        guard let self else { return }
        self.queue.async { self.relay.restartIfActive() }
      }
    }

    OnDestroy {
      if let o = self.foregroundObserver { NotificationCenter.default.removeObserver(o) }
      self.queue.async {
        self.sockets.values.forEach { $0.close() }
        self.sockets = [:]
        self.browser.stop()
        self.relay.stop()
      }
    }

    // MARK: Sockets

    AsyncFunction("open") { (id: String, endpoint: PinnedEndpointRecord, timeoutMs: Double, promise: Promise) in
      let target = endpoint.target
      self.queue.async {
        if self.sockets[id] != nil { promise.reject("io", "io"); return }
        let conn = PinnedConnection(target: target, queue: self.queue)
        self.sockets[id] = conn
        conn.connect(timeout: max(timeoutMs, 1) / 1000) { failure in
          if let failure {
            self.sockets[id] = nil
            promise.reject(failure.rawValue, failure.rawValue)
            return
          }
          conn.onLine = { [weak self] line in self?.sendEvent("onSocketLine", ["id": id, "line": line]) }
          conn.onClosed = { [weak self] error in
            self?.sockets[id] = nil
            var body: [String: Any] = ["id": id]
            if let error { body["error"] = error.rawValue }
            self?.sendEvent("onSocketClosed", body)
          }
          promise.resolve(nil)
          conn.startReading()
        }
      }
    }

    AsyncFunction("writeLine") { (id: String, line: String, promise: Promise) in
      self.queue.async {
        guard let conn = self.sockets[id] else { promise.reject("closed", "closed"); return }
        conn.sendLine(line) { failure in
          if let failure { promise.reject(failure.rawValue, failure.rawValue) } else { promise.resolve(nil) }
        }
      }
    }

    Function("close") { (id: String) in
      self.queue.async { self.sockets[id]?.close() }
    }

    // MARK: Bonjour

    Function("startBrowsing") { self.queue.async { self.browser.start() } }
    Function("stopBrowsing") { self.queue.async { self.browser.stop() } }

    // MARK: Localhost relay

    // Mirror ports (LocalhostRelay): Network.framework is enough, on every supported iOS version.
    Function("isLocalhostRoutingSupported") { () -> Bool in true }

    AsyncFunction("setLocalhostRoute") { (route: LocalhostRouteRecord?, promise: Promise) in
      let tunnel = route.map { TunnelRoute(endpoints: $0.endpoints.map(\.target), linkId: $0.linkId, token: $0.token) }
      let ports = route?.ports ?? []
      LinkLog.info(route.map { "route localhost : \($0.linkId) via \($0.endpoints.map { "\($0.host):\($0.port)" }.joined(separator: ", "))" } ?? "route localhost : aucune")
      self.queue.async {
        self.relay.setRoute(tunnel, ports: ports)
        promise.resolve(true)
      }
    }

    // Nothing to do: the mirror ports serve every WebView, private tabs included.
    AsyncFunction("attachToWebView") { (_: Int, promise: Promise) in promise.resolve(true) }

    Function("deviceName") { () -> String in UIDevice.current.name }
  }
}

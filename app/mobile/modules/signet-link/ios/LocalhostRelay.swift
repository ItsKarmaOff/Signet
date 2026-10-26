import Foundation
import Network

/// Route to the computer: where to reach it and with which credentials.
struct TunnelRoute {
  var endpoints: [PinnedTarget]
  let linkId: String
  let token: String
  /// Index of the last endpoint that answered (tried first).
  var preferred = 0
}

/// Opens a tunnel: pinned socket to the first endpoint that answers, `hello` line (tunnel role), wait for `ok`.
enum TunnelOpener {
  struct Opened { let connection: PinnedConnection; let endpointIndex: Int }

  static func open(route: TunnelRoute, port: Int, queue: DispatchQueue, completion: @escaping (Result<Opened, TunnelFailure>) -> Void) {
    let count = route.endpoints.count
    guard count > 0 else { completion(.failure(TunnelFailure(code: "unreachable"))); return }
    let order = (0..<count).map { (route.preferred + $0) % count }
    var lastCode = "unreachable"

    func attempt(_ n: Int) {
      guard n < order.count else { completion(.failure(TunnelFailure(code: lastCode))); return }
      let index = order[n]
      let conn = PinnedConnection(target: route.endpoints[index], queue: queue)
      var answered = false
      var answerTimer: DispatchWorkItem?

      func next(_ code: String) {
        guard !answered else { return }
        answered = true
        answerTimer?.cancel()
        lastCode = code
        conn.onLine = nil
        conn.onClosed = nil
        conn.close()
        attempt(n + 1)
      }

      conn.connect(timeout: 3) { failure in
        if let failure { lastCode = failure.rawValue; attempt(n + 1); return }
        conn.onClosed = { _ in next("io") }
        conn.onLine = { line in
          guard !answered else { return }
          answered = true
          answerTimer?.cancel()
          let type = (try? JSONSerialization.jsonObject(with: Data(line.utf8)) as? [String: Any])
          if type?["type"] as? String == "ok" {
            conn.onLine = nil
            conn.onClosed = nil
            completion(.success(Opened(connection: conn, endpointIndex: index)))
          } else {
            // The computer answered with a protocol error: no point trying the other addresses.
            conn.onClosed = nil
            conn.close()
            completion(.failure(TunnelFailure(code: type?["code"] as? String ?? "bad-request")))
          }
        }
        let hello: [String: Any] = ["type": "hello", "v": 1, "linkId": route.linkId, "token": route.token, "role": "tunnel", "port": port]
        let json = (try? JSONSerialization.data(withJSONObject: hello)).map { String(decoding: $0, as: UTF8.self) } ?? "{}"
        conn.sendLine(json) { f in if let f { next(f.rawValue) } }
        conn.startReading()
        let t = DispatchWorkItem { next("timeout") }
        answerTimer = t
        queue.asyncAfter(deadline: .now() + 5, execute: t)
      }
    }
    attempt(0)
  }
}

struct TunnelFailure: Error { let code: String }

/// "Mirror" ports: the phone listens on its own localhost (127.0.0.1 and ::1), at the ports that listen on the computer,
/// and passes each connection through a tunnel to the same port on the computer. The WebView therefore opens `http://localhost:5173`
/// as is, without a proxy: on a real iPhone, iOS never sends `localhost` to a proxy.
/// A port already taken on the phone is ignored. In the simulator nothing is relayed: it shares the Mac's localhost
/// (the relay would call itself there), and `localhost` already leads to the computer.
final class LocalhostRelay {
  private let queue: DispatchQueue
  private var route: TunnelRoute?
  private var listeners: [Int: [NWListener]] = [:]
  private var sessions: [ObjectIdentifier: RelaySession] = [:]
  private var lastErrors: [String: Date] = [:]
  /// Tunnel error to report to JS.
  var onTunnelError: ((Int, String) -> Void)?

  init(queue: DispatchQueue) { self.queue = queue }

  /// Sets the route and the relayed ports (route nil = full stop).
  func setRoute(_ newRoute: TunnelRoute?, ports: [Int]) {
    var next = newRoute
    // Same computer: keep the address that worked last.
    if let old = route, var r = next, r.linkId == old.linkId {
      r.preferred = min(old.preferred, max(r.endpoints.count - 1, 0))
      next = r
    }
    route = next
    #if targetEnvironment(simulator)
      let wanted = Set<Int>()
    #else
      let wanted = next == nil ? Set<Int>() : Set(ports.filter { (1...65535).contains($0) })
    #endif
    for (port, ls) in listeners where !wanted.contains(port) {
      ls.forEach { $0.cancel() }
      listeners[port] = nil
    }
    for port in wanted.sorted() where listeners[port] == nil { listen(port) }
    if next == nil {
      for s in sessions.values { s.end() }
      sessions = [:]
    }
    LinkLog.info(next == nil ? "mirrors: stopped" : "mirrors: localhost \(wanted.sorted().map(String.init).joined(separator: ", "))")
  }

  func stop() { setRoute(nil, ports: []) }

  /// Back in the foreground: iOS may have reclaimed the listening sockets during sleep, without notice.
  func restartIfActive() {
    guard route != nil, !listeners.isEmpty else { return }
    let ports = listeners.keys.sorted()
    LinkLog.info("mirrors: restarted on return to the foreground")
    for ls in listeners.values { ls.forEach { $0.cancel() } }
    listeners = [:]
    ports.forEach(listen)
  }

  private func listen(_ port: Int) {
    guard let p = NWEndpoint.Port(rawValue: UInt16(port)) else { return }
    var started: [NWListener] = []
    for host in [NWEndpoint.Host.ipv4(.loopback), NWEndpoint.Host.ipv6(.loopback)] {
      let params = NWParameters.tcp
      params.requiredLocalEndpoint = .hostPort(host: host, port: p)
      guard let l = try? NWListener(using: params) else { continue }
      l.newConnectionHandler = { [weak self] c in self?.accept(c, port: port) }
      l.stateUpdateHandler = { [weak self, weak l] st in
        guard let self, let l, case .failed(let error) = st else { return }
        LinkLog.info("mirrors: localhost:\(port) unavailable on \(host) (\(error))")
        l.cancel()
        self.listeners[port]?.removeAll { $0 === l }
      }
      l.start(queue: queue)
      started.append(l)
    }
    listeners[port] = started
  }

  private func accept(_ c: NWConnection, port: Int) {
    guard let route else { c.cancel(); return }
    let session = RelaySession(client: c, port: port, relay: self, queue: queue)
    sessions[ObjectIdentifier(session)] = session
    session.start(route: route)
  }

  fileprivate func finished(_ s: RelaySession) { sessions[ObjectIdentifier(s)] = nil }
  fileprivate func rememberEndpoint(_ index: Int) { route?.preferred = index }

  /// Avoids flooding JS when all of a page's connections fail for the same reason.
  fileprivate func reportError(port: Int, code: String) {
    let key = "\(port)/\(code)"
    if let last = lastErrors[key], Date().timeIntervalSince(last) < 2 { return }
    lastErrors[key] = Date()
    LinkLog.info("mirrors: tunnel to port \(port) refused (\(code))")
    onTunnelError?(port, code)
  }
}

/// A WebView connection on a mirror port, relayed as is through a tunnel to the same port on the computer.
private final class RelaySession {
  private let client: NWConnection
  private let port: Int
  private unowned let relay: LocalhostRelay
  private let queue: DispatchQueue
  private var tunnel: PinnedConnection?
  private var ended = false

  init(client: NWConnection, port: Int, relay: LocalhostRelay, queue: DispatchQueue) {
    self.client = client
    self.port = port
    self.relay = relay
    self.queue = queue
  }

  func start(route: TunnelRoute) {
    client.stateUpdateHandler = { [weak self] st in
      if case .failed = st { self?.end() }
    }
    client.start(queue: queue)
    TunnelOpener.open(route: route, port: port, queue: queue) { [weak self] result in
      guard let self, !self.ended else {
        if case .success(let opened) = result { opened.connection.close() }
        return
      }
      switch result {
      case .failure(let f):
        self.relay.reportError(port: self.port, code: f.code)
        // Almost always a web page: a readable HTTP response rather than a dropped connection.
        let body = Data("Signet could not reach localhost:\(self.port) on your computer (\(f.code)).".utf8)
        let header = "HTTP/1.1 502 Bad Gateway\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Length: \(body.count)\r\nConnection: close\r\n\r\n"
        self.client.send(content: Data(header.utf8) + body, completion: .contentProcessed { [weak self] _ in self?.queue.async { self?.end() } })
      case .success(let opened):
        self.relay.rememberEndpoint(opened.endpointIndex)
        self.tunnel = opened.connection
        self.pump()
      }
    }
  }

  /// Closes everything, only once: the tunnel immediately, the WebView after flushing what is left to write to it.
  func end() {
    guard !ended else { return }
    ended = true
    tunnel?.close()
    tunnel = nil
    let c = client
    c.send(content: nil, contentContext: .finalMessage, isComplete: true, completion: .contentProcessed { _ in c.cancel() })
    queue.asyncAfter(deadline: .now() + 2) { c.cancel() }
    relay.finished(self)
  }

  // MARK: - Byte relay

  private func pump() {
    guard let tunnel, !ended else { return }
    tunnel.onRaw = { [weak self] data, next in
      guard let self, !self.ended else { return }
      self.client.send(content: data, completion: .contentProcessed { [weak self] err in
        self?.queue.async {
          if err != nil { self?.end() } else { next() }
        }
      })
    }
    tunnel.onClosed = { [weak self] _ in self?.end() }
    tunnel.switchToRaw()
    readClient()
  }

  private func readClient() {
    client.receive(minimumIncompleteLength: 1, maximumLength: 64 * 1024) { [weak self] data, _, complete, err in
      guard let self, !self.ended, let tunnel = self.tunnel else { return }
      self.queue.async {
        guard !self.ended else { return }
        let finished = err != nil || complete
        if let data, !data.isEmpty {
          tunnel.send(data) { [weak self] f in
            if f != nil || finished { self?.end() } else { self?.readClient() }
          }
        } else if finished {
          self.end()
        } else {
          self.readClient()
        }
      }
    }
  }
}

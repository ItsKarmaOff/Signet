import CryptoKit
import Foundation
import Network

/// Where to reach the computer and which fingerprint to expect (mirror of `PinnedEndpoint` on the JS side).
struct PinnedTarget {
  let host: String
  let port: Int
  let serverName: String
  let certSha256: String
}

enum SocketFailure: String {
  case timeout, refused, unreachable
  case pinMismatch = "pin-mismatch"
  case tls, closed
  case lineTooLong = "line-too-long"
  case io
}

/// SHA-256 in base64url without "=" (same format as the QR code fingerprint).
func base64Url(_ digest: some Sequence<UInt8>) -> String {
  Data(digest).base64EncodedString().replacingOccurrences(of: "+", with: "-").replacingOccurrences(of: "/", with: "_")
    .replacingOccurrences(of: "=", with: "")
}

/// TLS socket with a pinned certificate: only the leaf certificate fingerprint matters (no authority, no hostname).
/// Everything happens on the serial `queue`. Two read modes: `\n` lines (≤ 64 KiB), then possibly raw bytes.
final class PinnedConnection {
  static let maxLine = 64 * 1024

  private enum State { case idle, connecting, ready, closed }
  private final class PinState { var mismatch = false }

  private let queue: DispatchQueue
  private let conn: NWConnection
  private let pin = PinState()
  private var state = State.idle
  private var timer: DispatchWorkItem?
  private var lastWaiting: SocketFailure?
  private var connectDone: ((SocketFailure?) -> Void)?

  // Reading
  private var buffer = Data()
  private var raw = false
  private var receiving = false
  private var rawBusy = false
  private var eof = false
  /// A received line (without `\n` or `\r`).
  var onLine: ((String) -> Void)?
  /// Raw bytes; call `next` when they are processed to receive the rest (backpressure).
  var onRaw: ((Data, @escaping () -> Void) -> Void)?
  /// Close after opening: called only once (error nil = normal close or `close()`).
  var onClosed: ((SocketFailure?) -> Void)?

  init(target: PinnedTarget, queue: DispatchQueue) {
    self.queue = queue
    let tls = NWProtocolTLS.Options()
    let sec = tls.securityProtocolOptions
    sec_protocol_options_set_min_tls_protocol_version(sec, .TLSv12)
    sec_protocol_options_set_tls_server_name(sec, target.serverName)
    let expected = target.certSha256
    let pin = self.pin
    // The standard verification is replaced: we compare the leaf certificate fingerprint.
    sec_protocol_options_set_verify_block(sec, { _, trust, complete in
      let secTrust = sec_trust_copy_ref(trust).takeRetainedValue()
      guard let chain = SecTrustCopyCertificateChain(secTrust) as? [SecCertificate], let leaf = chain.first else {
        pin.mismatch = true
        complete(false)
        return
      }
      let der = SecCertificateCopyData(leaf) as Data
      let ok = base64Url(SHA256.hash(data: der)) == expected
      if !ok { pin.mismatch = true }
      complete(ok)
    }, queue)
    let tcp = NWProtocolTCP.Options()
    tcp.noDelay = true
    tcp.enableKeepalive = true
    tcp.keepaliveIdle = 30
    let params = NWParameters(tls: tls, tcp: tcp)
    let port = NWEndpoint.Port(rawValue: UInt16(clamping: target.port)) ?? .any
    conn = NWConnection(host: NWEndpoint.Host(target.host), port: port, using: params)
  }

  // MARK: - Opening

  /// Calls `completion` only once: nil if the TLS handshake is done and the fingerprint matches.
  func connect(timeout: TimeInterval, completion: @escaping (SocketFailure?) -> Void) {
    guard state == .idle else { completion(.closed); return }
    state = .connecting
    connectDone = completion
    conn.stateUpdateHandler = { [weak self] st in self?.handle(st) }
    let t = DispatchWorkItem { [weak self] in self?.fail(self?.lastWaiting ?? .timeout) }
    timer = t
    queue.asyncAfter(deadline: .now() + timeout, execute: t)
    conn.start(queue: queue)
  }

  private func handle(_ st: NWConnection.State) {
    switch st {
    case .ready:
      guard state == .connecting else { return }
      state = .ready
      timer?.cancel()
      let done = connectDone
      connectDone = nil
      done?(nil)
    case .waiting(let err):
      // "Waiting" = no route yet (or refusal). Allowed until the timeout, except on refusal or a rejected certificate.
      let f = Self.map(err, mismatch: pin.mismatch)
      lastWaiting = f
      if f == .refused || f == .pinMismatch { fail(f) }
    case .failed(let err):
      fail(Self.map(err, mismatch: pin.mismatch))
    default:
      break
    }
  }

  private static func map(_ err: NWError, mismatch: Bool) -> SocketFailure {
    if mismatch { return .pinMismatch }
    switch err {
    case .posix(let code):
      switch code {
      case .ECONNREFUSED: return .refused
      case .ETIMEDOUT: return .timeout
      case .EHOSTUNREACH, .ENETUNREACH, .ENETDOWN, .EHOSTDOWN, .EADDRNOTAVAIL, .ENOTCONN: return .unreachable
      default: return .io
      }
    case .dns: return .unreachable
    case .tls: return .tls
    default: return .io
    }
  }

  private func fail(_ f: SocketFailure) {
    switch state {
    case .connecting:
      state = .closed
      timer?.cancel()
      conn.cancel()
      let done = connectDone
      connectDone = nil
      done?(pin.mismatch ? .pinMismatch : f)
    case .ready:
      close(f)
    default:
      break
    }
  }

  // MARK: - Closing

  /// Closes the socket. Idempotent. Before opening: `connect` ends in `closed`. After: `onClosed` once.
  func close(_ error: SocketFailure? = nil) {
    switch state {
    case .idle:
      state = .closed
      conn.cancel()
    case .connecting:
      state = .closed
      timer?.cancel()
      conn.cancel()
      let done = connectDone
      connectDone = nil
      done?(error ?? .closed)
    case .ready:
      state = .closed
      conn.cancel()
      let cb = onClosed
      onLine = nil
      onRaw = nil
      onClosed = nil
      buffer = Data()
      cb?(error)
    case .closed:
      break
    }
  }

  // MARK: - Writing

  func send(_ data: Data, completion: ((SocketFailure?) -> Void)? = nil) {
    guard state == .ready else { completion?(.closed); return }
    conn.send(content: data, completion: .contentProcessed { [weak self] err in
      self?.queue.async {
        if err != nil { self?.close(.io); completion?(.io) } else { completion?(nil) }
      }
    })
  }

  func sendLine(_ line: String, completion: ((SocketFailure?) -> Void)? = nil) {
    send(Data((line + "\n").utf8), completion: completion)
  }

  // MARK: - Reading

  func startReading() { pump() }

  /// Switches to raw bytes: what is already in the buffer goes to `onRaw`.
  func switchToRaw() {
    raw = true
    pump()
  }

  /// Dispatches the buffer, then asks the network for more.
  private func pump() {
    while state == .ready {
      if raw {
        if rawBusy { return }
        if buffer.isEmpty { break }
        let chunk = buffer
        buffer = Data()
        rawBusy = true
        onRaw?(chunk) { [weak self] in
          self?.queue.async {
            self?.rawBusy = false
            self?.pump()
          }
        }
        return
      }
      guard let nl = buffer.firstIndex(of: 10) else {
        if buffer.count >= Self.maxLine { close(.lineTooLong); return }
        break
      }
      let lineData = buffer[buffer.startIndex..<nl]
      buffer.removeSubrange(buffer.startIndex...nl)
      if lineData.count + 1 > Self.maxLine { close(.lineTooLong); return }
      var line = String(decoding: lineData, as: UTF8.self)
      if line.hasSuffix("\r") { line.removeLast() }
      onLine?(line)
    }
    guard state == .ready else { return }
    if eof && !rawBusy { close(nil); return }
    if !receiving && !eof { receive() }
  }

  private func receive() {
    receiving = true
    conn.receive(minimumIncompleteLength: 1, maximumLength: 64 * 1024) { [weak self] data, _, complete, err in
      guard let self else { return }
      self.queue.async {
        self.receiving = false
        guard self.state == .ready else { return }
        if let data, !data.isEmpty { self.buffer.append(data) }
        if err != nil {
          self.pump()
          self.close(.io)
          return
        }
        if complete { self.eof = true }
        self.pump()
      }
    }
  }
}

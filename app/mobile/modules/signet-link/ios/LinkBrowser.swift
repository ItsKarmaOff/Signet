import Foundation
import dnssd
import Network

/// Bonjour discovery of `_signet._tcp`: NWBrowser for the list and the TXT, NetService for the addresses (all of them, IPv4 first).
final class LinkBrowser {
  struct Found {
    let name: String
    let desktopId: String
    let version: Int
    let hosts: [String]
    let port: Int
  }

  private struct Entry {
    /// Fingerprint of the TXT seen by the browser (empty if it did not provide one: the resolution's TXT is read instead).
    let fingerprint: String
    var resolver: ServiceResolver?
    var found: Found?
  }

  private let queue: DispatchQueue
  private var browser: NWBrowser?
  private var entries: [String: Entry] = [:]
  private var running = false
  var onFound: ((Found) -> Void)?
  var onLost: ((String) -> Void)?

  init(queue: DispatchQueue) { self.queue = queue }

  func start() {
    if running {
      // Already browsing: return what is known.
      entries.values.compactMap(\.found).forEach { onFound?($0) }
      return
    }
    running = true
    startBrowser()
  }

  func stop() {
    running = false
    browser?.stateUpdateHandler = nil
    browser?.browseResultsChangedHandler = nil
    browser?.cancel()
    browser = nil
    entries.values.forEach { $0.resolver?.cancel() }
    entries = [:]
  }

  private func startBrowser() {
    let b = NWBrowser(for: .bonjourWithTXTRecord(type: "_signet._tcp", domain: "local."), using: .tcp)
    browser = b
    b.browseResultsChangedHandler = { [weak self] results, _ in self?.sync(results) }
    b.stateUpdateHandler = { [weak self, weak b] st in
      guard let self, let b, self.browser === b, self.running else { return }
      if case .failed = st {
        b.cancel()
        self.browser = nil
        self.queue.asyncAfter(deadline: .now() + 2) { [weak self] in
          guard let self, self.running, self.browser == nil else { return }
          self.startBrowser()
        }
      }
    }
    b.start(queue: queue)
  }

  /// Compares the network's full list with what we know: new and changed ones are resolved, vanished ones are reported.
  private func sync(_ results: Set<NWBrowser.Result>) {
    var current: [String: [String: String]] = [:]
    for r in results {
      guard case .service(let name, _, _, _) = r.endpoint else { continue }
      var dict: [String: String] = [:]
      if case .bonjour(let txt) = r.metadata { dict = txt.dictionary }
      // Several results for the same name (one per interface): keep the one that has a TXT.
      if current[name] == nil || !dict.isEmpty { current[name] = dict }
    }
    for name in entries.keys where current[name] == nil {
      entries[name]?.resolver?.cancel()
      let wasFound = entries[name]?.found != nil
      entries[name] = nil
      if wasFound { onLost?(name) }
    }
    for (name, dict) in current {
      let fp = dict.sorted { $0.key < $1.key }.map { "\($0.key)=\($0.value)" }.joined(separator: "&")
      if let old = entries[name], old.fingerprint == fp { continue }
      entries[name]?.resolver?.cancel()
      entries[name] = Entry(fingerprint: fp)
      resolve(name, txt: dict, attempt: 0)
    }
  }

  private func resolve(_ name: String, txt: [String: String], attempt: Int) {
    guard let entry = entries[name] else { return }
    let fp = entry.fingerprint
    let resolver = ServiceResolver(name: name, interface: 0, queue: queue) { [weak self] result in
      do {
        guard let self, let current = self.entries[name], current.fingerprint == fp else { return }
        switch result {
        case .success(let r):
          let dict = txt.isEmpty ? r.txt : txt
          guard let id = dict["id"], !id.isEmpty else { return }  // not a Signet computer
          let found = Found(name: name, desktopId: id, version: Int(dict["v"] ?? "") ?? 0, hosts: r.hosts, port: r.port)
          self.entries[name]?.found = found
          self.entries[name]?.resolver = nil
          self.onFound?(found)
        case .failure:
          // Retry a few times: resolution sometimes fails right after the service appears.
          guard attempt < 3 else { return }
          self.queue.asyncAfter(deadline: .now() + 2) { [weak self] in self?.resolve(name, txt: txt, attempt: attempt + 1) }
        }
      }
    }
    entries[name]?.resolver = resolver
    resolver.start()
  }
}

/// Resolution of a Bonjour service into addresses + port (NetService lives on the main run loop).
private struct Resolved { let hosts: [String]; let port: Int; let txt: [String: String] }

/// Resolution of a Bonjour service into addresses + port + TXT, using the dnssd API (all addresses, no run loop).
/// Everything happens on `queue`. `cancel()` releases the requests; no callback arrives afterwards.
private final class ServiceResolver {
  private let name: String
  private let interface: UInt32
  private let queue: DispatchQueue
  private var done: ((Result<Resolved, Error>) -> Void)?
  private var resolveRef: DNSServiceRef?
  private var addrRef: DNSServiceRef?
  private var port = 0
  private var txt: [String: String] = [:]
  private var v4: [String] = []
  private var v6: [String] = []
  private var timer: DispatchWorkItem?

  init(name: String, interface: UInt32, queue: DispatchQueue, done: @escaping (Result<Resolved, Error>) -> Void) {
    self.name = name
    self.interface = interface
    self.queue = queue
    self.done = done
  }

  func start() {
    let ctx = Unmanaged.passUnretained(self).toOpaque()
    let err = DNSServiceResolve(&resolveRef, 0, interface, name, "_signet._tcp.", "local.", { _, _, _, err, _, host, port, txtLen, txt, ctx in
      guard let ctx else { return }
      let me = Unmanaged<ServiceResolver>.fromOpaque(ctx).takeUnretainedValue()
      me.resolved(error: err, host: host.map { String(cString: $0) }, port: Int(UInt16(bigEndian: port)), txt: txt, txtLen: Int(txtLen))
    }, ctx)
    guard err == kDNSServiceErr_NoError, let ref = resolveRef else { finish(.failure(NSError(domain: "SignetLink", code: Int(err)))); return }
    DNSServiceSetDispatchQueue(ref, queue)
    arm(after: 8) { [weak self] in self?.finish(.failure(NSError(domain: "SignetLink", code: -1))) }
  }

  func cancel() {
    done = nil
    release()
  }

  private func release() {
    timer?.cancel()
    if let r = resolveRef { DNSServiceRefDeallocate(r); resolveRef = nil }
    if let r = addrRef { DNSServiceRefDeallocate(r); addrRef = nil }
  }

  private func arm(after seconds: Double, _ block: @escaping () -> Void) {
    timer?.cancel()
    let t = DispatchWorkItem(block: block)
    timer = t
    queue.asyncAfter(deadline: .now() + seconds, execute: t)
  }

  private func finish(_ result: Result<Resolved, Error>) {
    let cb = done
    done = nil
    release()
    cb?(result)
  }

  private func resolved(error: DNSServiceErrorType, host: String?, port: Int, txt: UnsafePointer<UInt8>?, txtLen: Int) {
    guard done != nil else { return }
    guard error == kDNSServiceErr_NoError, let host else { finish(.failure(NSError(domain: "SignetLink", code: Int(error)))); return }
    self.port = port
    // TXT: a sequence of "length + key=value" blocks.
    if let txt {
      var i = 0
      while i < txtLen {
        let n = Int(txt[i])
        i += 1
        if n > 0, i + n <= txtLen {
          let entry = String(decoding: UnsafeBufferPointer(start: txt + i, count: n), as: UTF8.self)
          if let eq = entry.firstIndex(of: "=") { self.txt[String(entry[..<eq])] = String(entry[entry.index(after: eq)...]) }
        }
        i += n
      }
    }
    if let r = resolveRef { DNSServiceRefDeallocate(r); resolveRef = nil }
    let ctx = Unmanaged.passUnretained(self).toOpaque()
    let err = DNSServiceGetAddrInfo(&addrRef, 0, interface, 0, host, { _, _, _, err, _, address, _, ctx in
      guard let ctx, let address else { return }
      let me = Unmanaged<ServiceResolver>.fromOpaque(ctx).takeUnretainedValue()
      me.address(error: err, address: address)
    }, ctx)
    guard err == kDNSServiceErr_NoError, let ref = addrRef else { finish(.failure(NSError(domain: "SignetLink", code: Int(err)))); return }
    DNSServiceSetDispatchQueue(ref, queue)
    arm(after: 5) { [weak self] in self?.finish(.failure(NSError(domain: "SignetLink", code: -2))) }
  }

  private func address(error: DNSServiceErrorType, address: UnsafePointer<sockaddr>) {
    guard done != nil, error == kDNSServiceErr_NoError else { return }
    var buf = [CChar](repeating: 0, count: Int(NI_MAXHOST))
    guard getnameinfo(address, socklen_t(address.pointee.sa_len), &buf, socklen_t(buf.count), nil, 0, NI_NUMERICHOST) == 0 else { return }
    let host = String(cString: buf)
    if address.pointee.sa_family == sa_family_t(AF_INET) {
      if !v4.contains(host) { v4.append(host) }
    } else if address.pointee.sa_family == sa_family_t(AF_INET6) {
      if !v6.contains(host) { v6.append(host) }
    }
    // Addresses arrive one by one: wait a short moment until no more come.
    arm(after: 0.4) { [weak self] in
      guard let self else { return }
      self.finish(.success(Resolved(hosts: self.v4 + self.v6, port: self.port, txt: self.txt)))
    }
  }
}

import ExpoModulesCore
import WebKit

/// Native content blocking and site data. Everything that touches WebKit goes through the main thread.
public class SignetWebModule: Module {
  /// Lists compiled in this session, by identifier.
  private var compiled: [String: WKContentRuleList] = [:]

  public func definition() -> ModuleDefinition {
    Name("SignetWeb")

    // Compiles (or reuses from cache) the lists bundled in the app (rules/manifest.json). Returns [{ identifier, count }].
    AsyncFunction("compileBundledRules") { (promise: Promise) in
      guard let manifest = Self.readManifest() else {
        promise.reject("E_NO_RULES", "No bundled rule list.")
        return
      }
      var summary: [[String: Any]] = []
      var wanted = Set<String>()
      let group = DispatchGroup()
      var failure: String?
      for entry in manifest {
        let identifier = "signet-\(entry.id)-\(entry.version)"
        wanted.insert(identifier)
        summary.append(["identifier": identifier, "count": entry.count])
        group.enter()
        self.ensureCompiled(identifier: identifier) {
          // Not in the store yet: read the file off the main thread, then compile.
          guard let url = Bundle.main.url(forResource: entry.file, withExtension: nil),
                let json = try? String(contentsOf: url, encoding: .utf8) else { return nil }
          return json
        } done: { error in
          if let error { failure = error }
          group.leave()
        }
      }
      group.notify(queue: .main) {
        if let failure { promise.reject("E_COMPILE", failure); return }
        // Previous versions are no longer needed.
        WKContentRuleListStore.default().getAvailableContentRuleListIdentifiers { ids in
          for id in ids ?? [] where id.hasPrefix("signet-") && !wanted.contains(id) {
            WKContentRuleListStore.default().removeContentRuleList(forIdentifier: id) { _ in }
          }
          promise.resolve(summary)
        }
      }
    }.runOnQueue(.main)

    // Compiles a list provided by JS (Safari content blocker JSON).
    AsyncFunction("compileRules") { (identifier: String, json: String, promise: Promise) in
      self.ensureCompiled(identifier: identifier) { json } done: { error in
        if let error { promise.reject("E_COMPILE", error) } else { promise.resolve(nil) }
      }
    }.runOnQueue(.main)

    AsyncFunction("removeRules") { (identifier: String, promise: Promise) in
      self.compiled[identifier] = nil
      WKContentRuleListStore.default().removeContentRuleList(forIdentifier: identifier) { _ in promise.resolve(nil) }
    }.runOnQueue(.main)

    // Attaches the (already compiled) lists to the WKWebView of the React view `viewTag`. Returns false if the WKWebView does not exist yet.
    AsyncFunction("attachRules") { (viewTag: Int, identifiers: [String], promise: Promise) in
      guard let web = self.findWebView(viewTag) else { promise.resolve(false); return }
      for id in identifiers {
        guard let list = self.compiled[id] else { continue }
        web.configuration.userContentController.remove(list)
        web.configuration.userContentController.add(list)
      }
      promise.resolve(true)
    }.runOnQueue(.main)

    AsyncFunction("detachRules") { (viewTag: Int, identifiers: [String], promise: Promise) in
      guard let web = self.findWebView(viewTag) else { promise.resolve(false); return }
      for id in identifiers {
        if let list = self.compiled[id] { web.configuration.userContentController.remove(list) }
      }
      promise.resolve(true)
    }.runOnQueue(.main)

    // Clears site data: cookies, cache, storage (localStorage, IndexedDB...) or everything. `since` in milliseconds.
    AsyncFunction("clearWebsiteData") { (types: [String], since: Double?, promise: Promise) in
      var set = Set<String>()
      for kind in types {
        switch kind {
        case "cookies": set.insert(WKWebsiteDataTypeCookies)
        case "cache":
          set.formUnion([WKWebsiteDataTypeDiskCache, WKWebsiteDataTypeMemoryCache, WKWebsiteDataTypeOfflineWebApplicationCache, WKWebsiteDataTypeFetchCache])
        case "storage":
          set.formUnion([
            WKWebsiteDataTypeLocalStorage, WKWebsiteDataTypeSessionStorage, WKWebsiteDataTypeIndexedDBDatabases,
            WKWebsiteDataTypeWebSQLDatabases, WKWebsiteDataTypeServiceWorkerRegistrations,
          ])
        case "all": set.formUnion(WKWebsiteDataStore.allWebsiteDataTypes())
        default: break
        }
      }
      let date = since.map { Date(timeIntervalSince1970: $0 / 1000) } ?? Date(timeIntervalSince1970: 0)
      if set.contains(WKWebsiteDataTypeCookies) {
        // react-native-webview shares cookies with the system's HTTP store.
        HTTPCookieStorage.shared.removeCookies(since: date)
      }
      WKWebsiteDataStore.default().removeData(ofTypes: set, modifiedSince: date) { promise.resolve(nil) }
    }.runOnQueue(.main)

    // Number of site data records (to display "N sites").
    AsyncFunction("websiteDataSummary") { (promise: Promise) in
      WKWebsiteDataStore.default().fetchDataRecords(ofTypes: WKWebsiteDataStore.allWebsiteDataTypes()) { records in
        promise.resolve(["records": records.count])
      }
    }.runOnQueue(.main)
  }

  // MARK: - Rules

  private struct Entry { let id: String; let version: String; let file: String; let count: Int }

  private static func readManifest() -> [Entry]? {
    guard let url = Bundle.main.url(forResource: "signet-rules-manifest", withExtension: "json"),
          let data = try? Data(contentsOf: url),
          let root = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
          let lists = root["lists"] as? [[String: Any]] else { return nil }
    return lists.compactMap { l in
      guard let id = l["id"] as? String, let version = l["version"] as? String, let file = l["file"] as? String else { return nil }
      return Entry(id: id, version: version, file: file, count: l["count"] as? Int ?? 0)
    }
  }

  /// Reuses the list from the store if present, otherwise compiles it from `source()`. Call on the main thread.
  private func ensureCompiled(identifier: String, source: @escaping () -> String?, done: @escaping (String?) -> Void) {
    if compiled[identifier] != nil { done(nil); return }
    let store = WKContentRuleListStore.default()!
    store.lookUpContentRuleList(forIdentifier: identifier) { found, _ in
      DispatchQueue.main.async {
        if let found {
          self.compiled[identifier] = found
          done(nil)
          return
        }
        DispatchQueue.global(qos: .utility).async {
          let json = source()
          DispatchQueue.main.async {
            guard let json else { done("List not found: \(identifier)"); return }
            store.compileContentRuleList(forIdentifier: identifier, encodedContentRuleList: json) { list, error in
              DispatchQueue.main.async {
                if let list { self.compiled[identifier] = list; done(nil) } else { done("\(error?.localizedDescription ?? "failure") (\(identifier))") }
              }
            }
          }
        }
      }
    }
  }

  // MARK: - WKWebView

  /// The React view from react-native-webview contains the WKWebView somewhere in its subviews.
  private func findWebView(_ tag: Int) -> WKWebView? {
    guard let root = appContext?.findView(withTag: tag, ofType: UIView.self) else { return nil }
    return Self.firstWebView(in: root)
  }

  private static func firstWebView(in view: UIView) -> WKWebView? {
    if let web = view as? WKWebView { return web }
    for sub in view.subviews {
      if let found = firstWebView(in: sub) { return found }
    }
    return nil
  }
}

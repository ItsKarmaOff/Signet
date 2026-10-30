# Extension APIs: probe results

Measured with the API probe (`app/desktop/tests/extensions/api-probe`, run by `app/desktop/tests/e2e/extension-api.spec.ts`) on macOS arm64, Electron 45.1.0, on 2026-10-30. The probe loads an MV3 extension requesting every permission of `src/shared/extension-api-catalog.ts` and runs in four contexts: service worker, popup page, options page, and an extension page inside an iframe of the options page.

Status of this document: the call results below were measured (copied from the generated report). The full member by member matrix (`test-results/extension-api-probe.md`) was generated but is not reproduced here, and the last runs were interrupted before it could be re-read, so anything about individual members or events is **not measured** unless stated.

## What each context gets

| context                     | `chrome`     | `browser`    | `browser === chrome` | `chrome` frozen |
| --------------------------- | ------------ | ------------ | -------------------- | --------------- |
| service worker              | yes          | yes          | yes                  | yes             |
| popup page                  | yes          | yes          | yes                  | yes             |
| options page                | yes          | yes          | yes                  | yes             |
| iframe in an extension page | yes (native) | yes (native) | **no**               | no              |

- Worker, popup and options behave the same: library namespaces plus Signet's (`proxy`, `privacy`, `alarms`, `offscreen`, `declarativeNetRequest`, `runtime.getContexts`), as logged by `[extension-apis] <id> worker: ...` and `page: ...`.
- Subframe gap confirmed: the iframe only has Chromium's native bindings. `commands`, `windows`, `cookies`, `privacy`, `contextMenus`, `notifications`, `topSites`, `downloads`, `webNavigation` are absent there, `proxy.settings.get` is rejected ("Access to extension API denied"), `tabs.getCurrent`, `extension.getViews` and `extension.isAllowedIncognitoAccess` are missing, `storage.sync` and `storage.managed` are "not available in this instance of Chrome", and `browser !== chrome`.
- `chrome.clipboard` exists natively in every context (ChromeOS API, added to the catalog as unsupported).

## Answers to the inventory's unverified items

| item                                       | result                                                                                                                                                                                                                                      |
| ------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `userScripts`                              | **Absent** in all contexts even with the permission. The namespace must come from the safety net or a real implementation.                                                                                                                  |
| `idle`                                     | Native and working: `idle.queryState(60)` resolves `"active"`.                                                                                                                                                                              |
| `power`                                    | Native and working: `requestKeepAwake` / `releaseKeepAwake` resolve.                                                                                                                                                                        |
| `dns`                                      | Native and working: `dns.resolve('localhost')` resolves.                                                                                                                                                                                    |
| `system.cpu / memory / display`            | Calls resolved without crashing when explicitly enabled (`SIGNET_PROBE_UNSAFE`). Return values not read.                                                                                                                                    |
| `system.storage` (`getInfo`, `onAttached`) | **Not conclusive.** First runs ended with the Electron process dying (SIGSEGV) around `system.storage.onAttached`, but those runs also overlapped with an extension reload, so the cause is not isolated. The probe skips these by default. |
| `printerProvider` events                   | Not conclusive either (one run died during the `onGetUsbPrinterInfoRequested` listener test, `onPrintRequested` survived). Skipped by default.                                                                                              |
| `storage.session`                          | Works: set then get round-trips in every context.                                                                                                                                                                                           |
| `storage.managed`                          | **Wrong, confirmed**: `get(null)` returns the extension's local data (alias of `local`).                                                                                                                                                    |
| `storage.sync`                             | Works but **aliased to local**: a key written to `local` is read from `sync`.                                                                                                                                                               |
| `runtime.setUninstallURL('')`              | Resolves.                                                                                                                                                                                                                                   |
| `runtime.requestUpdateCheck`               | Rejects with "Autoupdate is not enabled." (acceptable).                                                                                                                                                                                     |
| `activeTab`                                | Not measured (needs a button click on a real page).                                                                                                                                                                                         |
| `alarms` native vs Signet                  | Signet's version is installed in the worker (`alarms` listed in the preload report); create/clear/getAll work. Not distinguished from native by this probe.                                                                                 |
| native `declarativeNetRequest` vs Signet   | Signet's version is installed; `getEnabledRulesets` / `getDynamicRules` resolve `[]`. The user verified on 2026-10-30 that uBOL "no filtering" lifts blocking, so there is no double enforcement.                                           |
| `commands.getAll`                          | Works in worker, popup, options (lists the manifest command); absent in the iframe.                                                                                                                                                         |
| `runtime.getContexts`                      | Works (Signet's override) in the three contexts and the iframe.                                                                                                                                                                             |
| `extension.getViews`                       | Returns `[]` (library stub).                                                                                                                                                                                                                |
| `action.isEnabled`                         | Always `false` (native stub).                                                                                                                                                                                                               |

## Namespaces: measured by calls

Provided and answering: `runtime`, `storage` (local, session, sync, managed), `idle`, `alarms`, `permissions`, `tabs`, `windows`, `management`, `declarativeNetRequest`, `scripting`, `cookies`, `i18n`, `offscreen`, `privacy`, `proxy`, `contextMenus`, `notifications` (level `granted`), `webNavigation`, `action`, `power`, `dns`, `commands`.

Present but stubbed: `topSites.get` resolves `[]`, `downloads.search` resolves `undefined` (library stub), `extension.getViews` `[]`.

Absent (calling any member throws a TypeError): `userScripts`, `bookmarks`, `history`, `sessions`, `fontSettings`, `tts`, `sidePanel`, `tabGroups`, `browsingData`, `identity`, `contentSettings`, `declarativeContent`, `readingList`. `desktopCapture`, `pageCapture`, `search`, `omnibox` and the other namespaces without a harmless call are **not measured**.

## Gaps the safety net (next batch) must cover

1. Absent namespaces above: stubs gated by manifest permission, with constants and classes (`declarativeContent` classes, `tabGroups.TAB_GROUP_ID_NONE`, ...).
2. Everything in the iframe context: the preload does not run in extension subframes (use `registerPreloadScript` match filters).
3. `storage.managed` must be an empty read-only area; `storage.sync` should be separate from `local`.
4. Library events: `hasListener` / `hasListeners` exceptions. The probe records `ev!` per event in the report; not reproduced here (not measured in this document).
5. `downloads` is a stub resolving `undefined`; `action.isEnabled` is always false.
6. Optional permissions path (`permissions.contains` for an optional permission reports false; grants are not persisted).

## Surprises

- Reloading an unpacked extension while its worker is running the probe killed the whole Electron process in several runs. The unpacked mechanism watches the folder, and on macOS the file watcher replayed the files just copied, triggering a reload about 400 ms after loading. The spec therefore loads the probe with `session.extensions.loadExtension` directly. This is worth a separate look: a developer saving a file in a busy extension could crash Signet.
- The two sandboxed popup/options pages in tabs report no errors and run Signet's preload, as expected.
- `NAMESPACE`-less keys: only `clipboard` appears on `chrome` outside the catalog.

## Safety net (implemented after this probe)

Status: implemented and unit-tested, **not yet re-measured** by the probe (the next probe run should show the changes below).

- **Placeholders**: for every namespace of the catalog the extension may use (permission, optional permission or manifest key), missing methods report `chrome.<ns>.<member> is not supported in Signet` (`runtime.lastError` or rejection), `soft` methods resolve a neutral value, missing events accept listeners and never fire, and constants, enums and classes exist. Existing members are never replaced, except the two cases below. The list of placeholders is logged once per extension version in development (`[extension-apis] <id> stubbed: ...`).
- **Library events**: events whose `hasListener`/`hasListeners` threw "Method not implemented" get listener tracking (`fixedEvents` in the install report).
- **Native crashers** (catalog field `crashes`): `system.storage` (`ejectDevice`, `getAvailableCapacity`, `getInfo`, `onAttached`, `onDetached`) and the four `printerProvider` events are replaced by inert placeholders, because the probe saw the whole process die when listeners were added to them. `system.cpu`, `system.memory` and `system.display` stay native: their `getInfo` calls resolved when explicitly enabled. The probe no longer skips any of these by default, so a crash in the next run means the replacement did not apply.
- **storage.managed** is an empty read-only area in every context. `storage.sync` is unchanged (still an alias of local).
- **Subframes**: the preload and the call router accept any `chrome-extension://` frame (not only the top frame), except sandboxed pages (opaque origin). The iframe context should now match the others, with `browser === chrome`.
- **Unpacked extensions**: reloads are now conditional on a real file change, serialised and preceded by a clean unload (see SPEC.md). The probe still loads its extension directly.

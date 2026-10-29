# Signet: v1 specification

A macOS browser built on Electron, organized around a vertical sidebar.
The interface is in **French**.

Stack: Electron 45, electron-vite 6, React 19, TypeScript 6, lucide-react for icons, @ghostery/adblocker-electron for the ad blocker, emojibase-data for the emoji picker.

The contract between processes lives in `app/desktop/src/shared/types/` (`api.ts` for `SignetApi`) and `app/desktop/src/shared/ipc/contract.ts` (channels). **Do not change it** without saying so.
Import alias: `@shared/types`. The data types (state, spaces, tabs, settings) come from `@signet/shared/types`.

## Files

The paths below are the current ones (monorepo). The sections of this document sometimes name a file by its bare name (`store.ts`, `tabs.ts`…): look it up in the list. The full map, with the role of each folder, is in [ARCHITECTURE.md](ARCHITECTURE.md).

```
app/desktop/src/shared/types/        data contract and `SignetApi` (also published by @signet/shared/types)
app/desktop/src/shared/ipc/          IPC contract (contract.ts) and argument guards (validate.ts)
app/desktop/src/main/index.ts        startup, user agent, open-url, single instance, IPC registration
app/desktop/src/main/core/           store.ts (persistent state, state.json), history.ts, atomic.ts
app/desktop/src/main/model/          state rules (open, close, move, archive…), without Electron
app/desktop/src/main/browser/        Browser class (browser.ts, composition root) and its parts:
                                     views, layout, broadcast, overlay, peek, undo, window, tab-actions…
app/desktop/src/main/browser/tabs.ts      creates a WebContentsView for a tab and wires its events
app/desktop/src/main/browser/palette.ts   command bar engine (query, suggestions, URL)
app/desktop/src/main/windows/        little.ts (Little Signet), private.ts, pip.ts, settings.ts, library.ts
app/desktop/src/main/app/            menu.ts (menu and shortcuts), platform.ts, i18n.ts, appearance.ts…
app/desktop/src/main/ipc/            IPC handlers, one file per domain
app/desktop/src/preload/index.ts     exposes window.signet (SignetApi) via contextBridge, built from the contract
app/desktop/src/renderer/src/main.tsx   hash routing: '' → main window, '#overlay', '#little'…
app/desktop/src/renderer/src/...     components, grouped by domain (see Interface)
packages/shared/src/urls.ts          toUrl(), searchUrl(); site.ts: siteOf()
```

## Model

- **Tabs**: `tabs: Record<id, TabInfo>`. A tab lives in exactly one place: favorites (global), a space's `pinned` or `today`, or "detached" (Peek, Little Signet, never persisted).
- **Favorites**: a global list of tab ids, shared by all spaces. They have a `pinnedUrl`. A favorite can't be part of a split view.
- **Spaces**: name, emoji, hue, `pinned: Item[]`, `today: Item[]`, `activeTabId`. At least one space always exists. Initial state: a "Personal" space, 🏠, gradient of hues 250 then 0 (purple → red), grain 0.5, empty.
- **Split view**: an `Item` of type `split` holding 2 or 3 `tabIds`. The tabs of a split view don't appear anywhere else. When only one tab is left, the split view turns back into a tab `Item`.
- **Pinned tabs**: `pinnedUrl` = the URL at the time of pinning. "Closing" a pinned tab = unloading it (destroy the view, `loaded=false`) and resetting `url = pinnedUrl`. It stays in the list: its row therefore never collapses (no `.leaving` animation) and the ✕ button is called "Unload". **Optimistic selection**: when you click a tab or a space (sidebar, tab row), the interface shows the selection right away (`optimistic.ts`); it drops at the first update received after the main process replies, or 400 ms after the reply if main refused. Picking a space from the popover of the top tab bar (overlay process) is optimistic too: on receiving `space:switch`, main immediately sends `space:pending` (id) to the main window, before the deferred layout/state cycle; `optimistic.ts` adopts it, and it drops at the next update or after 1 s if main refused.
- **Sidebar width, elastic**: while the handle is dragged, the raw width beyond 180-420 is shown through `rubberband(overshoot, 160, 0.55)`. The IPC `sidebar:width(w, live)` with `live=true` relaxes the bound to 100-500 (±80) on the main side (transient width `liveSidebarWidth`, in the layout and the pushed state, never persisted). On release, `spring()` (≈0.3 s, critical, release velocity) brings it back to the bounded value, frame by frame, then a last non-`live` value is sent and persisted. A new grab interrupts the spring. Reduce motion: hard bound, no spring.
- **Today tabs**: archived automatically after `settings.archiveAfterHours` (12 by default) without activation, except the active tab. Checked every 5 minutes. Archiving = destroy the view, remove the tab, add it to `archive` (url, title, favicon, date; 500 max). Archives are searchable in the command bar.
- **Recently closed tabs**: a stack of 20 for ⇧⌘T (memory only, never written to disk). Each entry keeps the URL, title, icon, custom name/icon, `pinnedUrl`, the original place (`spaceId`, zone, `folderId`, rank) and the navigation history (`TabHistory`, restored by `navigationHistory.restore`). ⇧⌘T reopens the tab **in its place**: same space, same zone, same folder, same rank (clamped to the current list). Space gone → current space (a pinned tab becomes a Today tab, at the top); folder gone → end of the space's pinned tabs. What feeds the stack: ⌘W on a Today tab, "Delete Folder and Its Tabs", "Delete Space" (the first tab of the list on top of the stack; 20 at most).
- **Undoable bulk archive**: "Archive" (separator button or the `clear-today` command bar action) archives all the Today tabs of the current space, then main pushes `today:archived` `{id, count}` to the sidebar, which shows the notice "N tabs archived · Undo" (5 s, paused on hover/focus, ✕ or Esc closes it without undoing). `today:undo(id)` (`undoClearToday`) puts the tabs back (same ids, histories) below the tabs opened since, removes their entries from the archive and restores the previous active tab if the user hasn't picked another one. One level only: a new archive replaces the previous one. Automatic archiving (inactivity delay) can't be undone. When no sidebar is shown (tabs on top, compact mode, hidden sidebar), main doesn't send the event to the window: it creates a small transparent `WebContentsView` (route `#toast`, 384×72, centered at the bottom of the web card, below the overlay, above the pages) that renders the same notice (same enter/exit), and removes it when the notice has finished its exit (`toast:done`). It only covers its own area, follows window resizing (recomputed on every layout), and undo works in it.
- **Lazy views**: at startup no view exists. A `WebContentsView` is created when the tab is shown for the first time.
- **Automatic unloading** (sleep): every minute, a tab hidden for longer than `settings.tabSleepMinutes` (Settings › General; missing = 30; 0 = never) is unloaded like a "closed" pinned tab (view destroyed, `loaded=false`, history saved), but **without** touching its `url`: when you come back it reloads where it was, with its history. Never unloaded: shown tabs (split view panes included), tabs that play or recently played sound, the one in the mini player, Peek, detached tabs (Little Signet); nothing is unloaded during a download or a tab drag and drop.
- **Hidden interface views**: the compact mode toolbar is only created if the mode is on; the floating sidebar is prewarmed 1.5 s after the sidebar is hidden (and created on hover otherwise); the find bar is prewarmed 4 s after the window opens (and created on demand by ⌘F); the ⌘Q bubble is prewarmed 5 s after launch (and created on demand by ⌘Q). Early sends are queued by `UiView`.

### Persistence (`store.ts`)

`app.getPath('userData')/state.json`, written with a 500 ms delay after each change and synchronously on `before-quit`. Content: tabs (id, url, title, favicon, pinnedUrl, lastActive, muted), favorites, spaces, activeSpaceId, sidebarVisible, sidebarWidth (240 by default, bounded 180-420), archive, settings, window bounds. Field `version: 1`. Tolerant read: missing or corrupt file → initial state.
Detached tabs (Peek, Little Signet) are never written.

### History (`history.ts`)

`history.json`, `Map<url, {url, title, favicon, visits, lastVisit}>`, 5,000 entries max (the oldest are dropped), deferred write of 2 s. Recorded on `did-navigate` and `did-navigate-in-page` (main frame), and the title is updated on `page-title-updated`. `about:`, `data:` and `devtools:` are ignored.

## Main window and layout

`BrowserWindow`: `titleBarStyle: 'hidden'`, `trafficLightPosition: { x: 16, y: 16 }`, 1280×820 by default (bounds restored), `minWidth 700`, `minHeight 480`, `vibrancy: 'under-window'`, `visualEffectState: 'active'`, `backgroundColor: '#00000000'`. Its `webContents` loads the interface (route `''`): sidebar and background.

Layout computation, redone on every state change and on window `resize`:

- `W, H = win.getContentBounds()` for width and height; `pad = 8`.
- Sidebar visible: content area `x = sidebarWidth`, otherwise `x = pad`. `y = pad`, `width = W - x - pad`, `height = H - 2*pad`.
- Sidebar hidden: `win.setWindowButtonVisibility(false)`, shown again when it comes back.
- Shown tabs: those of the item that holds the active space's `activeTabId` (1 tab, or all the tabs of the split view). None if `activeTabId` is null.
- N panes side by side, `gap = 8`, equal widths.
- Each view: `setBorderRadius(10)`, white background. Views that are no longer shown are removed from `contentView` (not destroyed). Keep a `Set` of attached views.
- The snapshot's `panes` = rects and `focused` (tab = `activeTabId`).
- Stacking order: tab views, then the overlay if open, then the Peek view. (Re-adding a view that is already a child brings it to the front.)

When the active tab changes: `lastActive = Date.now()`, keyboard focus on its `webContents`.

### State broadcast

Every change calls `changed()`, which batches the sends (`setImmediate`): recomputes the layout, sends `state` (Snapshot) to the main `webContents`, sends `little:state` to each Little Signet window and schedules the save.

## Tabs (`tabs.ts`)

Create a view: `new WebContentsView({ webPreferences: { sandbox: true, contextIsolation: true, nodeIntegration: false } })`.
Events to wire on its `webContents` and reflect on `TabInfo`, then `changed()`:

- `page-title-updated`, `page-favicon-updated` (first favicon), `did-start-loading` / `did-stop-loading`
- `did-navigate`, `did-navigate-in-page` (main frame) → `url`, `canGoBack/canGoForward` via `webContents.navigationHistory`, history
- `audio-state-changed` → `audible`
- `setWindowOpenHandler(({ url, disposition, features }))`:
  - `disposition === 'new-window'` and non-empty `features` → popup (often an OAuth sign-in): `{ action: 'allow' }` with a 520×720 window
  - `disposition === 'new-window'` without `features` (⇧ click) → Peek, `deny`
  - `background-tab` (⌘ click) → new Today tab right after the current item, without activating it, `deny`
  - otherwise → new activated Today tab, `deny`
- `will-navigate`: if `settings.peekExternalLinks`, the tab has a `pinnedUrl`, it's the main frame and `siteOf(new url) !== siteOf(current url)` → `preventDefault()` and open in Peek
- `before-input-event`: Esc in the Peek view → close Peek
- `context-menu` → native menu in French:
  - on a link: Open in New Tab, Open in Peek, Open in Split View, Copy Link
  - on an image: Open Image in New Tab, Copy Image (`copyImageAt`), Save Image (`downloadURL`)
  - selected text: Copy, Search for "30-character excerpt" (new tab)
  - editable field: Undo, Redo, Cut, Copy, Paste, Select All (roles)
  - otherwise: Back, Forward, Reload
  - always last: separator + Inspect (`inspectElement`)

## URL (`urls.ts`)

- `toUrl(input)`: scheme present (`https://`, `about:`, `file:`, `data:`, `view-source:`…) → as is; `localhost[:port]` or IPv4 → `http://`; no space and looks like a domain (`x.yz`, optional port and path) → `https://`; otherwise `null`.
- `searchUrl(q)`: `https://www.google.com/search?q=…&hl=<language>`, `hl` taken from the first site language (`setSearchLanguage`, called by `applyLanguages`): the base language (`en-US` → `en`), except `zh-…` and `pt-…` which keep their region. Without `hl`, Google picks the language from the IP address (in Korea: Korean) and ignores `Accept-Language`.
- `siteOf(url)`: host name without `www.`, reduced to the approximate registrable domain (last 2 labels, 3 if the second to last is ≤ 3 letters and the last is 2 letters, like `co.uk`).

## Command bar (`palette.ts` + overlay)

The overlay is a single `WebContentsView` per main window, created at startup, transparent background (`setBackgroundColor('#00000000')`), which loads the `#overlay` route. Open = add it on top of the stack, covering the whole window, send `overlay:show` and give it keyboard focus. Close = remove it, send `overlay:hide` and give focus back to the active tab. Palette, site panel and popover play a 120 ms exit (main keeps the view in place, `OVERLAY_EXIT_MS`); **except** for a click outside the panel (`closeOverlay(true)`): the view is removed at once (the user is already acting elsewhere, it must not swallow their next click). A click on the overlay during an exit (Esc, action) also ends it immediately. Entries and exits of the glass surfaces (scrim, palette, site panel, popover, the sidebar's `.ex` panels) are **CSS transitions** (`@starting-style` for the entry, `closing` class for the exit; scale 0.96 + opacity, fade only with reduced motion): closing mid-entry starts from the current value, reopening mid-exit picks up from there (except the palette, which is mounted fresh).

Modes:

- `new-tab` (⌘T, "New Tab" button): empty field.
- `edit-url` (⌘L, click on the URL): field prefilled with the active tab's URL, text selected; submitting navigates in the active tab (if there is none, new tab).
- `split` (⌃⇧=, "Split View" action): the chosen result joins a split view with the active tab. An existing tab that is chosen is moved into the split view. Max 3 panes. If the active tab is a favorite or there is none: the result simply opens in a new tab.

`paletteQuery(q, mode)` (synchronous on the main side, fast), max ~12 results. Empty q: in `new-tab` mode, the 6 most recent open tabs of the active space, then 4 useful actions. Otherwise every candidate gets a relevance score and the list is sorted on it (ties keep insertion order; Enter runs the top row):

- Internal `signet:` addresses (2000), then an exact local alias (1500), then a typed address (`toUrl(q)` gives a `url` result, 900).
- Prefix of a local alias name (115).
- Pages (open tabs, history, archive): match quality 0-100 from `core/rank.ts`: start of the host ignoring `www.` = 100, start of a host label = 80, start of the title = 60, word start in the title = 45, inside the host = 40, word start in the path = 30, anywhere else in title or URL = 15 (accent and case insensitive). History score = quality x (0.35 + 0.65 x frecency); frecency (0-1) mixes visit count (saturates at ~50 visits) and recency (halves every 10 days). Open tabs (`tab`, subtitle "Go to tab · space name") score quality + 10, so they come before the same address in history (an address appears only once: tab, history, archive). Archives score quality x 0.4.
- The `search` row "Search for "q" on <engine>" (engine chosen in Settings › General or during onboarding, Google by default; suggestions come from the same engine) scores 45 when q is not an address: it sits above weak matches (a generic word, a site seen once long ago) and below a site you really visit ("youtube" puts youtube.com first, "Search youtube" right after). With no history match it stays first.
- At most 5 history and archive rows are kept. For free text the `search` row is never below the second row: if a page outranks it, it goes right after the best result. For free text without a `/`, when the best row is a deep page matched by the start of its host, the site's root comes first (an existing row, else a history lookup, else a synthesized `host` row) so the row, the host-only inline completion and Enter agree. In the interface the matched part of the title is shown in bold on page rows and suggestions (same rules as the ranking, `shared/highlight.ts`).
- Actions whose label contains q score 40 (prefix) or 30, always below the search row.

`paletteSuggest(q)`: `net.fetch('https://duckduckgo.com/ac/?type=list&q=…')` → 4 `suggestion` results max; network error → `[]`. The interface calls it after 150 ms without typing and adds the suggestions right after the search row (after the first result if that row is further down), so they never take the top spot; while typing, the previous suggestions that still match the start of the input stay shown until the new ones arrive.

Actions (id → label, shortcut shown):

- `new-space` New Space (opens the form in the sidebar via `space:edit` with `spaceId: null`)
- `split` Split View (⌃⇧=) → reopens the bar in `split` mode
- `unsplit` Leave Split View
- `toggle-sidebar` Show / Hide Sidebar (⌘S)
- `pin` Pin / Unpin Tab (⌥⌘P; ⌘D adds the page to bookmarks, see v16)
- `favorite` Add to Favorites / Remove from Favorites
- `copy-url` Copy URL (⇧⌘C)
- `little` New Little Signet Window (⌥⌘N)
- `clear-today` Archive Today's Tabs
- `close-tab` Close Tab (⌘W)
- `reopen` Reopen Closed Tab (⇧⌘T)
- `reload` Reload (⌘R), `devtools` Developer Tools (⌥⌘I)
- `toggle-peek-links` Open External Links from Pinned Tabs in Peek: on/off
- `default-browser` Set as Default Browser (`app.setAsDefaultProtocolClient('http')` and `'https'`; only works in the packaged app, say so in the subtitle)
- one `space:<id>` action "Go to Space <name>" per space

`paletteExecute(result, mode)` closes the overlay, then: `url`/`search`/`suggestion`/`history`/`archived` → open the URL according to the mode; `tab` → activate (switching space if needed) or, in `split` mode, move it into the split view; `action` → run it.

## Peek

Only one Peek at a time in the main window. Open: create a detached tab with its view, open the overlay in `peek` mode (dimmed background + frame + buttons), then add the web view on top.
Card: `width = min(W - 160, 1100)`, `height = H - 96`, centered horizontally, `y = 64`. Web view: `setBorderRadius(12)`. The payload's `card` lets the interface draw the shadow and place the buttons above it on the right (Close ✕, Open as tab ⤢).

- Click on the dimmed background, Esc (in the overlay or in the page) → close: destroy the view and the tab.
- `expandPeek` → remove the overlay, insert the tab at the top of the active space's Today tabs, activate it.
- The payload title updates: send `overlay:show` again when the title changes.
- Opening the command bar during a Peek closes the Peek first.

## Little Signet (`little.ts`)

A small window to open a link quickly (⌥⌘N, external links received through `open-url`, second launch with a URL).
`BrowserWindow` 760×820, `titleBarStyle: 'hidden'`, `trafficLightPosition: { x: 14, y: 14 }`, `vibrancy: 'under-window'`, cascaded by 24 px from the previous one. Loads the `#little` route (44 px bar: back, forward, title and domain, "Copy URL" button, "Open in Signet" button). A detached tab whose view is placed at `{ x: 6, y: 44, width: W - 12, height: H - 50 }`, `setBorderRadius(10)`. ⌥⌘N without a URL → the window opens on the command bar… too complicated in v1: open `https://www.google.com`.

- `littleOpenInMain` (from the bar's `webContents`, find the window through `BrowserWindow.fromWebContents(event.sender)`): remove the view from the small window, insert the tab at the top of the active space's Today tabs, activate it, show and focus the main window, close the small window without destroying the tab.
- Closing the window destroys the tab.
- Menu shortcuts (⌘W, ⌘R, ⌘[, ⌘], ⌘L…) act on the Little Signet if it is the frontmost window.

## Private windows (`private.ts`, route `#private`)

Opening: ⇧⌘N (File menu), the command bar's `private` action, "Open in Private Window" in a link's context menu. Several windows possible, cascaded by 24 px.

- **Session**: one per window, `session.fromPartition('private-<uuid>')` (without `persist:`, so in memory). Two private windows share nothing. Configured by `configurePrivateSession`: languages, blocker, downloads, `signet:` pages, swipe to navigate; **no extensions**; permissions in an in-memory table owned by the session (never in `sitePermissions`). On close: `clearStorageData`, `clearCache`, `clearAuthCache`, blocker removed.
- **Tabs**: detached (`store.detached`, never persisted or visible in the sidebar), session inherited (`inheritSession`). Nothing in history (`history.record`/`update` ignored), no ⇧⌘T, never unloaded. Closing the last tab closes the window; closing the window forgets all its tabs.
- **Links**: `window.open`, `target=_blank`, "Open in New Tab", search for the selection, image → new tab right after, in the same window. Popups with `features` (OAuth) stay real windows (same session). Peek, split view, Boost and mini player are absent from the context menu.
- **Downloads**: the file goes to Downloads; the entry (`private: true`) is never written to state.json and leaves the list when the window closes (once finished).
- **Layout**: the main window's (`settings.tabLayout`), followed live (⌃⇧S, sidebar width); `PrivateState.sidebarWidth` is the sidebar width, or null for tabs on top.
  - Tabs on top: 40 px row (traffic lights at `{16, 13}`, `tchip day` chips, + button, "Private" badge), card at `{ x: 8, y: 44, width: W - 16, height: H - 52 }`.
  - Sidebar (width `sidebarWidth`): 44 px header (traffic lights at `{16, 16}`, badge on the right), `row` rows for the tabs (shown tab on `--active`), "New tab" row. No spaces, favorites, pinned tabs or folders. Card at `{ x: sidebarWidth, y: 8, width: W - sidebarWidth - 8, height: H - 16 }`.
- **Interface**: the card carries a 40 px bar with Back, Forward, Reload and an address field (domain / title at rest, full address when editing, Enter → `privateNavigate`, Esc cancels). The web view is placed under this bar, over the rest of the card. Empty tab: no view, a welcome screen that explains what is kept or not, address field focused. Fixed hue `--h: 285` and a more tinted background. The bar asks for its state at startup (`private:get`), then receives `private:state`.
- **Shortcuts** in a frontmost private window: ⌘T new tab, ⌘W close tab, ⌘L address field, ⌃Tab / ⌃⇧Tab / ⇧⌘] / ⇧⌘[ neighbor tab, ⌘1…⌘9; ⌘R, ⌘[, ⌘], zoom and developer tools target the shown tab. The other commands bring back the main window. ⌘F does nothing.
- Channels: `private:open`, `private:get`, `private:new-tab`, `private:activate`, `private:close`, `private:navigate`; events `private:state`, `private:focus-address`. Check: `SIGNET_PRIVATECHECK=<folder>` (isolated profile).

## Menu and shortcuts (`menu.ts`)

The application menu accelerators work whichever pane has focus. Each command targets `BrowserWindow.getFocusedWindow()`: main window or Little Signet.

- **Signet**: About, Hide, Hide Others, Quit (⌘Q)
- **File**: New Tab ⌘T, New Little Signet Window ⌥⌘N, New Private Window ⇧⌘N, Reopen Closed Tab ⇧⌘T, Close Tab ⌘W (in Little Signet: close the window), Close Window ⇧⌘W
- **Edit**: roles undo, redo, cut, copy, paste, pasteAndMatchStyle, selectAll
- **View**: Show/Hide Sidebar ⌘S, Reload ⌘R, Force Reload ⇧⌘R, Zoom In ⌘+ (and ⌘=), Zoom Out ⌘-, Actual Size ⌘0 (on the active tab, `setZoomLevel`), Full Screen (role), Developer Tools ⌥⌘I
- **Navigate**: Back ⌘[, Forward ⌘], Open Address ⌘L, Copy URL ⇧⌘C, Pin/Unpin ⌥⌘P, Bookmark ⌘D, Split View ⌃⇧=, Next Tab ⌃Tab or ⇧⌘], Previous Tab ⌃⇧Tab or ⇧⌘[ (goes through favorites, pinned tabs, then Today tabs of the active space), Tab 1 to 8 ⌘1…⌘8 and Last Tab ⌘9 (`gotoTabNumber`: sidebar order from top to bottom, that is favorites, pinned, Today; tabs in a closed folder don't count, nor does any folder with tabs on top), Next Space ⌥⌘→, Previous Space ⌥⌘←, Space 1 to 9 ⌃1…⌃9
- **Window**: windowMenu role

## Startup (`index.ts`)

- `app.setName('Signet')`.
- User agent: remove ` Electron/x` and ` signet/x` (case insensitive) from `app.userAgentFallback`, and reduce the version like Chrome and CEF do (`Chrome/156.0.0.0` instead of `Chrome/156.0.8078.48`): identity.ts `reducedUserAgent`.
- `requestSingleInstanceLock()`; `second-instance`: if an http(s) URL is in argv → Little Signet, otherwise focus the main window.
- `open-url` (can arrive before `ready`: queue it) → Little Signet.
- Permissions (`session.defaultSession.setPermissionRequestHandler`): allow without asking `clipboard-sanitized-write`, `fullscreen`, `pointerLock`; for the rest, `dialog.showMessageBox` in French ("example.com is asking for access to the camera and microphone / to your location / to send notifications / to the clipboard…", Allow / Deny), decision remembered for the session by `origin|permission`. Deny by default anything not listed in the labels. A request from a tab that isn't shown waits until it is before opening the dialog; if the tab is closed in the meantime, the request is denied without being remembered.
- A tab's load failure (`did-fail-load` of the main frame, except `ERR_ABORTED`): the engine's error page is replaced by a clear page (title, host, cause in plain language based on the code, raw code as a fallback, "Try again" button = `location.reload()`). The tab keeps the original address, history records nothing. Process crash (`render-process-gone`, except a normal exit): "This page crashed" page with "Reload"; ⌘R / the Reload button reopen the original address (`reloadContents`, tabs.ts). Texts: `app/desktop/src/main/web/errorpage.ts`.
- `activate`: recreate the main window if it doesn't exist. `window-all-closed`: don't quit on macOS.
- All the `ipcMain.handle` of the `SignetApi` contract. Channels: the method name in kebab-case prefixed with the domain, for example `tab:activate`, `tab:close`, `tab:navigate`, `tab:back`, `tab:forward`, `tab:reload`, `tab:toggle-mute`, `tab:reset-pinned`, `tab:menu`, `item:move`, `space:switch`, `space:create`, `space:update`, `space:menu`, `sidebar:toggle`, `sidebar:width`, `today:clear`, `pane:focus`, `split:remove`, `palette:open`, `overlay:close`, `palette:query`, `palette:suggest`, `palette:execute`, `peek:expand`, `little:open-in-main`, `little:copy-url`, `state:get`. Pushed events: `state`, `space:edit`, `overlay:show`, `overlay:hide`, `little:state`. **The preload and main must use exactly these names.**
- Paths: preload `join(__dirname, '../preload/index.js')`; interface in dev `process.env.ELECTRON_RENDERER_URL + '#route'`, in prod `loadFile(join(__dirname, '../renderer/index.html'), { hash })`.

### Sidebar context menus (native, built in main)

- Tab (`tab:menu`): Pin / Unpin, Add to Favorites / Remove from Favorites, Split View with Active Tab (if it isn't the active one), Remove from Split View (if it is in one), Duplicate, Copy URL, Move to Space ▸ (submenu of the other spaces), separator, Close.
- Space (`space:menu`): Edit Space… (sends `space:edit`), Delete Space… (confirmation, disabled if only one is left; the detail reminds that the tabs reopen with ⇧⌘T and, for an isolated profile, that cookies and sign-ins are permanently erased).

### Drag and drop (`item:move`)

Remove the source from where it is (favorites, pinned or Today of any space), then insert it at `target.index` (clamped) in the target zone. To `favorites`: only a tab, `pinnedUrl = url`. To `pinned`: `pinnedUrl = url` (for each tab of a split view). To `today`: `pinnedUrl` removed. If the moved tab was active in the source space and the space changes, the source space picks another active tab. Watch the index when moving down within the same list.

### Choosing the active tab after a close

The next item in the same list, otherwise the previous one, otherwise the first pinned tab, otherwise `null` (empty background).

## Interface (React)

The stylesheets are in `app/desktop/src/renderer/src/styles/` (`index.css` imports them in cascade order; it was a single `styles.css` sheet originally). System font (`-apple-system`), 13 px. Light and dark mode (`prefers-color-scheme`). Space color: CSS variable `--h` = the space's hue; sidebar background = gradient `hsl(var(--h) 70% 86% / .78)` → `hsl(calc(var(--h) + 35) 65% 80% / .78)` in light mode, dark versions (`20%` lightness) in dark mode, 300 ms transition on space change. The whole background of the main window takes this color (the web views sit on it with their rounded corners). No hard borders: hovers in `rgb(255 255 255 / .35)` (light) or `/.08` (dark), active items in 70 % white with a slight shadow.

### Main window (route `''`)

Sidebar on the left, width `sidebarWidth`, from top to bottom:

1. Top bar, 44 px, `-webkit-app-region: drag`, 78 px left margin for the traffic lights: buttons (in `no-drag`) Sidebar, Back, Forward, Reload.
2. URL pill: domain of the active tab (without `www.`), padlock if https; click → `openPalette('edit-url')`.
3. Favorites: a 4-column grid of square tiles with the favicon, active tile highlighted; hidden if there are none, but the drop zone is visible during a drag.
4. Space header: name only (the emoji is in the bottom bar); right click → `showSpaceMenu`.
5. Pinned tabs. A pinned tab whose URL has left its `pinnedUrl` shows a small clickable "/" bar that calls `resetPinned`.
6. Separator with an "Archive" button on hover (`clearToday`, undoable through the "N tabs archived · Undo" notice).
7. "+ New tab" row → `openPalette('new-tab')`.
8. Today tabs (scrolling if needed).
9. Bottom bar: one button per space (its emoji; the active one is more opaque), "+" → new space form. The space buttons are also drop zones (drop = move to the top of that space's Today tabs).

Tab row: 32 px, 16 px favicon (globe icon by default, pulsing while loading), speaker icon between the favicon and the title if `audible` or `muted` (click → `toggleMute`; on the left, the cross that appears on hover doesn't shift it), truncated title, ✕ button on hover (`closeTab`, with no reserved space when hidden). Click → `activateTab`. Right click → `showTabMenu`. Middle click → close.

Favicon loading (everywhere in the interface, `Favicon` component): the interface loads the site's address; on failure (site headers, sign-in required, no network), it asks main (`favicon:fallback`, app/desktop/src/main/web/favicons.ts), which fetches it with the session's cookies (default, then isolated spaces) and returns a data URL. Successes are kept in memory, failures retried after a minute (before: one failure showed the globe until restart).
Split view row: a rounded container with one segment per tab, separated by a thin line; the clicked segment → `focusPane` (and activates the space); the active segment is highlighted.

HTML5 drag and drop: rows and tiles are `draggable`, source in `dataTransfer` (JSON of `DragSource`); when hovering a row, insert before or after depending on the vertical half (2 px indicator line); empty area at the end of the list = append at the end; drop → `moveItem`.

Resize handle: a 6 px strip on the right edge of the sidebar (`cursor: col-resize`), `setSidebarWidth` while dragging.

Space form: a small panel in the sidebar (never above the web views, which would hide it): name field, emoji field, 10 hue swatches (0, 25, 45, 90, 150, 190, 215, 250, 285, 330), Cancel / Create or Save. Opened by the "+", or by `onEditSpace`.

Content area (right of the sidebar): nothing when panes are shown (the web views cover it), except a 2 px focus ring (space color, more saturated) around the active pane when there is more than one, drawn at `rect` grown by 3 px with a 13 px radius. When `panes` is empty: a centered empty state, "No open tabs" and a "New Tab ⌘T" button.

Sidebar hidden: the content area takes the full width; no floating sidebar in v1.

### Overlay (route `#overlay`)

`html, body` transparent. Listens to `onOverlayShow` / `onOverlayHide`; clears its state on close.

- **Command bar**: scrim `rgb(0 0 0 / .12)` over the whole window (click → `closeOverlay`), 640 px wide panel centered at 18 % from the top, 14 px radius, translucent light background (`backdrop-filter: blur(30px)`) with an edge in the space color; 52 px field with autofocus (text preselected in `edit-url` mode), discreet mode label ("Split view" in `split` mode). List: icon by type (favicon for tabs, history and archives; magnifier for search and suggestions; globe for URL; action icon), title, gray subtitle, shortcut on the right for actions. ↑ ↓ to navigate, Enter to run, Esc to close, mouse hover = selection. Query on every keystroke; suggestions after 150 ms. Entry animation: fade + scale 0.98 → 1 in 120 ms.
- **Peek**: scrim `rgb(0 0 0 / .35)` (click → `closeOverlay`), drop shadow under `card` (the web view sits on top), two 28 px round buttons above the card on the right (`card.y - 36`): ⤢ "Open as tab" (`expandPeek`) and ✕ (`closeOverlay`), and the page title in white on the left on the same line. Esc → `closeOverlay`.

### Little Signet (route `#little`)

Background tinted by `hue` (same gradient as the sidebar). 44 px bar in `drag`, 76 px left margin: Back, Forward (`goBack()`/`goForward()` without argument: main targets the tab of the sending window), favicon + title + domain in the middle, "Copy URL" and "Open in Signet" buttons on the right.

## v2

New contract items: `Settings.adblock`, `DownloadInfo`, `FindState`, `Snapshot.downloads`, `Snapshot.sidebarFloating` and `SignetApi` methods (sections "Floating sidebar", "Find in page", "Downloads").
New channels: `sidebar:hover`, `find:query`, `find:close`, `download:open`, `download:reveal`, `download:cancel`, `downloads:clear`; pushed event `find:state`.
New interface routes: `#find` and `#sidebar`.

`store.ts`: soft migration. A v1 `state.json` has neither `settings.adblock` (→ `true`) nor `downloads` (→ `[]`): fill them with the default values on load, without changing `version`. Downloads are persisted (30 max); on load, those left `progressing` or `paused` become `interrupted`.

### View stacking order

Tab views → find bar → floating sidebar → overlay → Peek view.

### Find in page

- One "find" `WebContentsView` per main window, created at startup like the overlay, transparent background, route `#find`, 360×48.
- Position: top right of the active pane, `x = rect.x + rect.width - 360 - 12`, `y = rect.y + 12`. Recomputed on every layout.
- **Edit** menu (after the roles): Find… ⌘F, Find Next ⌘G, Find Previous ⇧⌘G. No effect if no tab is active or if the frontmost window is a Little Signet.
- ⌘F: opens the bar on the active tab (increments `session`, keeps that tab's last query if there is one, otherwise empty) and gives it keyboard focus. If it is already open, gives it focus again and selects the text (new `find:state` send with the same `session`).
- `find(query, opts)`: empty query → `stopFindInPage('clearSelection')`, `active = matches = 0`. Otherwise `findInPage(query, { forward, findNext })` on the active tab. The `found-in-page` event (`result.finalUpdate`) updates `active` / `matches` → `find:state`.
- ⌘G / ⇧⌘G: if the bar is open and the query isn't empty, next / previous match; otherwise same as ⌘F.
- `closeFind()` (Esc, ✕ button): `stopFindInPage('clearSelection')`, remove the view, give focus back to the tab.
- The bar closes by itself when the active tab changes, when the space changes or when the tab navigates to another page (`did-navigate`, not `did-navigate-in-page`).
- `#find` interface: `html, body` transparent; a rounded pill (radius 12, blurred translucent background, soft shadow, edge in the hue) that fills the view: magnifier icon, field (autofocus, text selected on each new state received with focus), counter "3 of 12" or "No results" (gray, soft red if 0 with a query), ↑ ↓ and ✕ buttons. Typing → `find(q, { forward: true, findNext: false })`; Enter → `findNext: true, forward: true`; ⇧Enter → `forward: false, findNext: true`; Esc → `closeFind()`. New `session` → reset the field with `query`.

### Downloads

- `session.defaultSession.on('will-download')`: save straight into the downloads folder without a dialog, unique name (`name (1).ext`, `name (2).ext`…). Create a `DownloadInfo` at the top of the list (30 max) and update it on `updated` (bytes, `paused` if `item.isPaused()`, `interrupted`) and `done` (`completed`, `cancelled`, `interrupted`). To avoid flooding the interface, limit the `changed()` calls caused by `updated` to one every 250 ms.
- Keep the `DownloadItem`s alive in a `Map<id, DownloadItem>` for `cancelDownload`.
- Downloads folder (Settings › General): `settings.downloadDir` (absolute path, chosen with "Change…" in a system "Choose a folder" dialog; "Default" clears it). Missing, or folder gone (disk unplugged, folder deleted): `app.getPath('downloads')`.
- "Ask where to save each file" (`settings.askDownloadPath`, missing = no): no `setSavePath`, `item.setSaveDialogOptions({ defaultPath })` on the folder's unique path; Electron opens its save dialog. The entry takes the chosen path as soon as it is known; dialog cancelled → the entry disappears from the list.
- Dock: `win.setProgressBar(fraction)` for all running downloads (−1 when there are none left, `2` = indeterminate if a size is unknown). At the end of a successful download: `app.dock?.downloadFinished(path)`.
- `openDownload` → `shell.openPath`; `revealDownload` → `shell.showItemInFolder`; if the file no longer exists, do nothing. `clearDownloads` removes everything except running downloads.
- A download triggered from a tab that never showed a page (opened just for the file, URL `about:blank` or empty title): close that Today tab automatically.
- Interface: in the sidebar header, to the left of the "New folder" button (which therefore doesn't move when it appears), a downloads button (`Download` icon), visible only if there is at least one download. During a download, a progress ring around the icon. A dot in the space's hue signals a finished download the user hasn't seen yet (state local to the interface: most recent id seen when the panel was opened).
- Click → panel in the sidebar (same principle as the space form: above the bottom bar, never above the web views): title "Downloads", "Clear" button, list. Each row: icon by extension (`FileText`, `FileImage`, `FileArchive`, `FileVideo`, `FileAudio`, `File`), name truncated in the middle, subtitle ("12.4 MB of 80 MB", "Done · 80 MB", "Cancelled", "Interrupted"), thin progress bar during the download. Click on a finished row → `openDownload`; magnifier button → `revealDownload`; ✕ button during the download → `cancelDownload`. Sizes in French units (o, Ko, Mo, Go, decimal comma).
- The panel closes on a click outside it or on Esc.

### Ad blocker

- `@ghostery/adblocker-electron` (already installed, in `dependencies`). Engine `ElectronBlocker.fromPrebuiltAdsAndTracking(net.fetch, { path: <userData>/adblock.bin, read: fs.promises.readFile, write: fs.promises.writeFile })` for the disk cache.
- Loaded at startup in the background (don't delay the window opening). `settings.adblock` true → `enableBlockingInSession(session.defaultSession)`. An error (offline on first launch) must not break anything: log it, retry at the next toggle.
- Cosmetic blocking relies on a preload injected by the library. Check in its code whether it is compatible with `sandbox: true` on the tab views. If it isn't, don't disable the sandbox: keep only network blocking and say so.
- Command bar action `toggle-adblock`: "Ad Blocker: on / off" → `enableBlockingInSession` / `disableBlockingInSession`, persisted.
- Update the engine if the cache is more than 7 days old (reload from the network in the background).

### Floating sidebar

- When `sidebarVisible` is false, the 8 px strip on the left edge of the main window (route `''`) detects hover (`mouseenter`) → `setSidebarHover(true)`.
- Main then shows a "sidebar" `WebContentsView` (created at startup, route `#sidebar`, same preload) at `{ x: 6, y: 6, width: sidebarWidth, height: H - 12 }`, `setBorderRadius(12)`, transparent background, and sets `sidebarFloating = true`. It sends it `state` like the main window. The traffic lights stay hidden.
- `#sidebar` interface: the same `Sidebar` component, on a solid background (the space gradient, almost opaque since a web view has no vibrancy) with a light 1 px edge; the document's `mouseleave` → `setSidebarHover(false)`. Don't reserve room for the traffic lights in the top bar when the route is `#sidebar`.
- Main hides the floating sidebar in three cases only: `setSidebarHover(false)` (after 250 ms, cancelled if `true` comes back in the meantime), ⌘S (the sidebar becomes visible normally again), opening the overlay. Activating a tab from the floating sidebar doesn't hide it: the mouse leaving does.
- The interface doesn't call `setSidebarHover(false)` while the space form or the downloads panel is open in the floating sidebar.
- The background (almost opaque glass) is carried by the sliding element (`.float-sidebar`, and likewise `.float-toolbar` for the compact mode toolbar), never by the view's page, which stays transparent: the panel arrives in one piece with its content, instead of appearing empty in place before the content slides into it. Frame by frame check: `SIGNET_FLOATCHECK=<folder>` (with `SIGNET_SEED=1`, isolated profile).
- During a drag and drop that starts from the floating sidebar, `mouseleave` must not close it (ignore `mouseleave` during a `drag`).

## v3

No AI feature.

New contract items: `Item` of type `folder`, `DropTarget.folderId`, `DragSource` of type `folder`, `Settings.appIcon`, `APP_ICONS`, `Boost`, `BoostEditorState`, `image` on `action` results, and `SignetApi` methods (sections "Pinned folders" and "Boosts").
New channels: `folder:create`, `folder:rename`, `folder:toggle`, `folder:menu`, `boost:save`, `boost:zap`, `boost:stop-zap`, `boost:unzap`, `boost:delete`; pushed events `folder:rename` (main → interface, rename request) and `boost:state`. The `folder:rename` channel is used both ways: `ipcMain.handle('folder:rename')` for `renameFolder`, `webContents.send('folder:rename')` for `onRenameFolder`.
New route: `#boost`.

Store migration (without changing `version`): `settings.appIcon` → `'aurore'`, `boosts` → `{}`.

### Pinned folders

- A folder only exists in `pinned`, at the root or inside another folder (free nesting, display depth limited to 6). Neither favorites nor Today tabs contain folders. Any tab that enters a folder gets a `pinnedUrl`.
- All the code that walks the lists (`locate`, `extract`, `moveItem`, `repick`/`pickNext`, `displayedTabIds`, `cycleTab`, command bar search, space deletion, `itemTabIds`) must descend recursively into folders. Write tree utilities once (walk, find parent and index, remove, insert) and use them everywhere.
- Dragging a folder: only to another position in the pinned tabs (same space or another); a drop in favorites or Today tabs is ignored; a folder can't be dropped into itself or one of its descendants.
- Create: `FolderPlus` button always visible in the space header, command bar action `new-folder` "New Folder", and in the context menu of a pinned or Today tab, "Add to Folder ▸" (submenu: the space's folders by their path "Parent / Child", then "New Folder"). A new folder is called "New folder", is open, and the interface immediately puts its name in edit mode (`folder:rename`).
- Folder context menu (`showFolderMenu`): Rename, New Folder Inside, separator, Delete Folder (its items move up to its place in the parent), Delete Folder and Its Tabs (confirmation, tabs destroyed).
- Folder row: `Folder` / `FolderOpen` icon, name, discreet chevron on hover; click → `toggleFolder`; double-click on the name (which first restores the open/closed state from before the first click: `toggleFolder(id, open)`) or a `folder:rename` request → inline edit field (Enter confirms, Esc cancels, blur confirms; empty name → unchanged). Children are indented by 14 px. Short opening animation.
- If the active tab is in a closed folder (or one of its closed subfolders), it still shows right under the row of the highest-level closed folder, indented, so the active tab is always visible.
- Drag and drop: on a folder row, the top quarter = before, the bottom quarter = after (at the parent's level), the middle = into the folder at the end (`folderId`, `index = items.length`) with the row highlighted. In an open folder, the insertion indicators carry its `folderId`.

### Import

Import of spaces, pinned tabs, folders and favorites from other installed browsers, through command bar actions that only appear if the source exists. The format of each source and the matching rules are described in `app/desktop/src/main/features/importers/FORMATS.md`: it is the only place in the repo, along with the code in that folder and the labels of the import actions, where these browsers are named. 2,000 tabs maximum per import, created unloaded.

### Boosts

Per-site customization, applied to all tabs (including Peek and Little Signet) whose `siteOf(url)` matches.

- Store: `boosts: Record<site, Boost>`.
- Applying: on each tab's `dom-ready` and on every change to a Boost, for each affected view: `removeInsertedCSS(previous key)` then, if the Boost is on, `insertCSS(css + hidden element rules)`, remembering the new key (`Map<webContents.id, key>`). Rule for a hidden element: `<selector> { display: none !important; }`.
- Emojis in dark mode: if the CSS contains the `signet:dark` block, add `[data-signet-emoji] { filter: invert(1) hue-rotate(180deg) !important; }` and run `EMOJI_SCRIPT` (app/desktop/src/main/features/boost/emoji.ts, isolated world 1007), which wraps each color emoji in a `<span data-signet-emoji>`; the original text node keeps the start of the text, and a `MutationObserver` splits again when the page rewrites or removes it. Without the block, `EMOJI_STOP_SCRIPT` restores the original text. Ignored: `script`, `style`, `textarea`, `option`, SVG, editable areas.
- Opening the editor: command bar action `boost` "Customize <site>" (subtitle "Boost active" if one exists; only if the active tab is http(s)), View menu "Customize This Site…" ⌥⌘B, and the "Customize This Site…" entry in the pages' context menu. One window per site, reused if it is already open.
- Window: `BrowserWindow` 400×620, `minHeight 420`, `titleBarStyle: 'hidden'`, `trafficLightPosition: { x: 14, y: 14 }`, `vibrancy: 'under-window'`, route `#boost`, same preload. `boost:state` sent on load (with a queue like the other views) and on every change.
- `#boost` interface: header in `drag` (76 px left margin) "Boost" + site; On switch; preset chips that add or remove a CSS block delimited by `/* signet:<id> */ … /* /signet:<id> */` comments: "Dark mode" (`html { filter: invert(1) hue-rotate(180deg); } img, video, picture, canvas, iframe, svg image { filter: invert(1) hue-rotate(180deg); }`), "System font" (`* { font-family: -apple-system, system-ui, sans-serif !important; }`), "Reading width" (`body { max-width: 860px !important; margin-inline: auto !important; }`), "No animations" (`*, *::before, *::after { animation: none !important; transition: none !important; }`); CSS area in a monospace font (Tab inserts two spaces, automatic save 400 ms after the last keystroke via `saveBoost`); "Hidden elements" section with the "Hide elements" button (`Crosshair` icon, disabled if `hasTab` is false), which becomes "Done (Esc)" during `zapping`, and the list of selectors with ✕; at the bottom "Delete Boost" with inline confirmation.
- "Hide" tool: target = the main window's active tab if its site matches, otherwise the first loaded tab of the site. Main brings the main window to the front and activates that tab, then calls `executeJavaScriptInIsolatedWorld(1001, [{ code }], true)` in a loop: the script shows a highlight (2 px outline and a light scrim that follow the hovered element, a small label with the tag), intercepts the next click in the capture phase (`preventDefault`, `stopPropagation`) and returns a promise resolved with a selector, or `null` on Esc or on a call to `window.__signetZapStop()`. Each selector is added (no duplicates), applied at once and pushed into the state, then the loop restarts. `stopZap` runs `window.__signetZapStop?.()` in the same isolated world. Building the selector: `#id` if the id is unique and doesn't look generated (no run of 4 or more digits); otherwise a chain of at most 5 ancestors, each `tag` + up to 2 stable classes (no digits or hash pattern like `css-1x2y3z`) + `:nth-of-type()` if needed, until `document.querySelectorAll(selector).length === 1`.

### App icon

- Drawing: defined in `app/desktop/scripts/icons/generate.mjs`. A bookmark (book ribbon with a split tail) hangs from the top edge, on the left, with three lines of text on the right: the sidebar and the page. The ribbon starts above the canvas and the squircle clips it, so it always touches the top edge, even on enlarged or shrunk mobile icons. Don't change the drawing, only the colors.
- Variants (background: 3 stops of the diagonal gradient; ribbon: 3 stops from left to right, with a white highlight; lines: color and opacity):
  - `aurore` (default): background `#2a3864` `#1f2a4b` `#141b33`; ribbon `#f0645e` `#d63a3f` `#b42a33`, highlight .22; lines `#4a5c8a`; shadow `#060a18` .50, glow .10
  - `ocean`: background `#2563eb` `#0891b2` `#2dd4bf`; ribbon `#fde68a` `#fbbf24` `#f59e0b`, highlight .35; white lines .50; shadow `#0c2d5e` .40, glow .22
  - `corail`: background `#fb923c` `#f43f5e` `#e11d74`; ribbon `#ffffff` `#fff1f4` `#ffe1e8`; white lines .50; shadow `#7f1d1d` .40, glow .22
  - `menthe`: background `#059669` `#10b981` `#a3e635`; ribbon `#ffffff` `#effdf5` `#d9f9e6`; white lines .50; shadow `#064e3b` .40, glow .22
  - `nuit`: background `#0b0b14` `#1e1b4b` `#312e81`; ribbon `#c4b5fd` `#a78bfa` `#8b5cf6`, highlight .30; lines `#6366f1` .70; black shadow .50, glow .08
  - `papier`: background `#ffffff` `#f8f7fb` `#efedf5`; ribbon like `aurore`; lines `#cfcbdc`; shadow `#1d1d1f` .18, no glow, black edge at .06
  - `graphite`: background `#52525b` `#27272a` `#18181b`; ribbon `#fafafa` `#e4e4e7` `#a1a1aa`; lines `#71717a`; black shadow .45, glow .08
  - `or`: background `#1c1917` `#292524` `#44403c`; ribbon `#fde68a` `#f59e0b` `#b45309`, highlight .30; lines `#78716c`; black shadow .45, glow .08
- Generation: `npm run icons` (scripts in `app/desktop/scripts/icons/`) writes `app/desktop/resources/icons/svg/<id>.svg` from the drawing, renders `resources/icons/<id>.png` at 1024×1024 (rendered by Electron offscreen: invisible `BrowserWindow`, `capturePage`, transparent background), and produces `build/icon.icns` (`aurore` variant) with `iconutil` from an `.iconset` (16, 32, 128, 256, 512 at @1x and @2x). The generated files are committed.
- Packaging: `mac.icon` = `build/icon.icns`; `extraResources` copies `resources/icons/*.png` into `icons/`.
- Runtime: at startup and on every change of `settings.appIcon`, `app.dock?.setIcon(nativeImage.createFromPath(...))`; folder `join(app.getAppPath(), 'resources/icons')` in development, `join(process.resourcesPath, 'icons')` in the packaged app.
- Choice: `icon:<id>` actions "App Icon: <Name>", subtitle "Current" for the active variant, `image` = 64 px preview as a data URL (computed once, `nativeImage.resize` then `toDataURL`). They appear when the input contains "icône", "icone", "icon" or a variant name, not with an empty input. The command bar shows `image` at 22 px with rounded corners in place of the action icon.
- Known limit (macOS): the Finder and Launchpad icon stays the default variant; only the Dock icon changes at runtime.

## Settings

A single window (reused if it is already open): `BrowserWindow` 780×600, `minWidth 640`, `minHeight 460`, `titleBarStyle: 'hidden'`, `trafficLightPosition: { x: 16, y: 18 }`, `vibrancy: 'sidebar'`, route `#settings`, same preload. Opening: Signet menu › "Settings…" ⌘,; command bar action `settings` "Settings"; ⚙ button (`Settings`) in the sidebar's bottom bar, to the left of the spaces.
New channels: `settings:open`, `settings:get`, `settings:update`, `settings:default-browser`, `settings:import`, `settings:open-boost`, `settings:clear-data`; pushed event `settings:info` (on page load, with a queue, and on every `changed()` while the window is open; ignore sends identical to the previous one).
`updateSettings` merges the patch, applies the effects (ad blocker, Dock icon…) reusing the existing code, then `changed()`. The equivalent command bar actions stay in place.

`#settings` interface: navigation column on the left (200 px, in `drag` at the top with room for the traffic lights, transparent background to let the vibrancy through) and content on the right on a plain background, section title at the top. Sections:

- **General**: "Archive Today tabs after" (picker: 6 h, 12 h, 24 h, 3 days, 7 days, never = 0 → main no longer archives if 0); switch "Open external links from pinned tabs in Peek"; "Default browser": status + "Make default" button (disabled with a note "Available in the installed app" if `packaged` is false).
- **Appearance**: grid of app icons (64 px previews, name below, the selection has a ring in the hue); discreet note "The Dock icon changes right away; the Finder icon stays Aurore."
- **Privacy**: switch "Block ads and trackers"; "Clear data" block with checkboxes (History: n entries, Archived tabs: n, Downloads list: n, Cache, Cookies and site data) and a "Clear…" button that asks for inline confirmation. Main side: history emptied and saved, archive emptied, downloads list emptied except running ones, `session.defaultSession.clearCache()`, `clearStorageData()` for cookies and site data.
- **Import**: one row per source with its name, "Import" if `available`, otherwise "Not found on this Mac" in gray; a note about the Full Disk Access some sources require. These are the only labels in this window that name other browsers (they come from `importSources`, not from the interface code).
- **Boosts**: list of Boosts (site, on or not, number of hidden elements, date), a switch per row (`saveBoost(site, { enabled })`), "Edit" button (`openBoost`), delete button (`deleteBoost`, inline confirmation). Empty state: explanation and the ⌥⌘B shortcut.
- **Shortcuts**: read-only table of all shortcuts, grouped (Tabs, Navigation, Spaces, Sidebar, Split view, Search, Windows). The list comes from a constant array in the interface, to be kept in sync with menu.ts.
- **About**: large icon of the active variant, "Signet", version, link to the repo https://github.com/ItsKarmaOff/Signet (opened in a new tab of the main window).

Style consistent with the rest (dark mode, lucide-react, soft hovers), native-like controls: rounded switches, pickers, secondary buttons.

## v4: sober visual redesign

Replaces the "colorful gradient" style with a neutral, calm interface. The golden rule: color is an accent, never a background. Remove **all** hue gradients (sidebar, main window, Little Signet, floating sidebar, Boost, Settings, overlay). The rules in this section take precedence over the visual descriptions of the previous sections; the behavior doesn't change.

### Colors

Single CSS tokens in `:root`, redefined in dark mode (`prefers-color-scheme: dark`):

- `--bg` window background: let the vibrancy through, with a neutral veil (light: `rgb(236 236 239 / .72)`; dark: `rgb(44 43 48 / .72)`).
- `--surface` cards and panels (light `#ffffff`, dark `#1f1f23`), `--surface-2` hovers and tiles (light `rgb(0 0 0 / .045)`, dark `rgb(255 255 255 / .06)`), `--border` (light `rgb(0 0 0 / .08)`, dark `rgb(255 255 255 / .08)`).
- `--text` (light `#1d1d1f`, dark `#ececf0`), `--text-2` secondary (light `#6e6e73`, dark `#9b9aa3`), `--text-3` tertiary.
- `--accent`: `hsl(var(--h) 70% 58%)` in light mode, `hsl(var(--h) 75% 68%)` in dark mode. Used only for accents (focus, selection ring, switches, active space dot).
- `--active` active tab background: dark `rgb(0 0 0 / .38)`, light `#ffffff` with `box-shadow: 0 1px 2px rgb(0 0 0 / .08), 0 0 0 .5px rgb(0 0 0 / .06)`.
- Windows: `vibrancy: 'sidebar'` for the main one and the Little Signets (instead of `under-window`).

### Main window layout

- Content column to the right of the sidebar: a **44 px toolbar** at the top (on the window background, `drag` area), then the **web card** below. Main offsets the views: `area.y = 44`, `area.height = H - 44 - 8`. The whole web card (one or more views) keeps its 4 rounded corners (radius 10).
- Toolbar (in the main window renderer, buttons in `no-drag`): on the left, sidebar button (only when it is hidden, after 78 px reserved for the traffic lights), Back, Forward, Reload (a cross while loading); then the address as "**domain** / page title" (domain in `--text`, "/ title" in `--text-3`, truncated; discreet padlock if https; click → `openPalette('edit-url')`). On the right: Boost button (`Rocket` icon, accent if a Boost is on for this site), and nothing else. In split view, the toolbar follows the focused pane.
- Sidebar (`--bg` background, no card):
  1. 44 px header in `drag`: 78 px for the traffic lights, active space name in bold 13 px (no emoji) (right click → space menu), then on the right the downloads button (progress ring, unseen dot) and the "New folder" button on hover.
  2. No URL pill or navigation buttons in the sidebar (they are in the toolbar).
  3. Favorites: **3-column** grid, 44 px tall tiles, radius 10, `--surface-2` background, `--border` edge, 18 px favicon centered; active tile in `--active`.
  4. Pinned tabs, with folders as **tinted cards**: background `hsl(hue 60% 50% / .16)` (dark) or `/ .12` (light), radius 10, 4 px padding; header: folder icon, name in color `hsl(hue 70% 62%)` (dark) or `hsl(hue 60% 42%)` (light), chevron; children inside. Without `hue`: `--surface-2` background, name in `--text`. Subfolders: same principle, with no double background if the hue is the same.
  5. Thin `--border` separator (same gap above and below; without pinned tabs, the empty list takes no room and the favorites lose their bottom margin), then "+ New tab" in `--text-2`, then the Today tabs.
  6. Tab row: 32 px, radius 8, 16 px favicon, 13 px title in `--text`; hover `--surface-2`; active `--active`. Close button on hover only.
  7. Bottom bar (36 px): Settings button (gear, `--text-3`) on the left; in the middle, **each space's emoji** (SpaceStrip.tsx: 28 px cells, dimmed 15 px emoji, the active space in full color on a `--sel` background, hover → bubble with the name); on the right "+" new space. The emojis stay drop zones. At most 5 emojis per page (4 if the row is narrower than 140 px): beyond that, the row is split into pages you swipe on the trackpad, like the spaces (1:1 tracking, hard stop at the first and last page, 10 px fade on the side where another page waits). When the active space changes, its page comes on screen with a spring that starts from the displayed position (interruptible by a gesture). A single page: the gesture on the row switches space like elsewhere in the sidebar.
- Background of the window and empty areas: `--bg`. Empty state of the web card: a `--surface` card with the text and the "New Tab ⌘T" button.

### Folder color

- New optional `hue` field on folders. Folder context menu: "Color ▸" submenu with None, then 8 named hues (Red 0, Orange 28, Yellow 48, Green 140, Turquoise 180, Blue 215, Purple 265, Pink 330), check mark on the current hue. A new folder has no hue. Neither do imported folders.

### Other views

- Command bar: `--surface` panel (slightly translucent with blur in dark mode), `--border` edge, soft shadow, no hue in the background; selected row in `--surface-2` with a 2 px accent bar on the left.
- Peek: scrim `rgb(0 0 0 / .45)`, round `--surface` buttons.
- Little Signet: 44 px bar on `--bg` (vibrancy), no gradient.
- Floating sidebar: almost opaque `--surface` background, `--border` edge, shadow.
- Find bar, space form, emoji picker, downloads panel, Boost, Settings: same tokens, no background hue. The accent stays for switches, selection rings and focus.

### Visual check

Extend `SIGNET_CAPTURE` (folder `app/desktop/src/main/dev/`: `capture.ts`, `seed.ts`, `probes.ts`) to capture the main window **with a sample state**: if `SIGNET_SEED=1`, before opening the window, replace the in-memory state (never on the normal profile: only if `--user-data-dir` is passed) with 2 spaces, 4 favorites, pinned tabs including an open blue-tinted folder of 3 tabs and a red folder of 1 tab, 5 Today tabs with one active, on simple public sites (wikipedia.org, example.com, github.com, news.ycombinator.com, lemonde.fr, w3.org). Captures in light and dark mode (`nativeTheme.themeSource`).

## v5: compact, media, advanced split view, advanced spaces, library

Four batches developed in parallel by four people. The contract (`app/desktop/src/shared/types/`) is already extended for all of them; each batch only implements its `SignetApi` methods in the preload and `index.ts`. Store migration without changing `version`: `settings.compactMode` → `false`, `settings.autoPip` → `true`, `settings.routes` → `[]`, `sitePermissions` → `{}`, `Snapshot.toolbarFloating` → `false`, `dividers` → `[]`, `media` → `null`, `boostedSites` → computed. `pruneItems` (tree.ts) must keep `layout`, `sizes` of split views and `customTitle`, `customIcon` of tabs.
Channels: method name in kebab-case prefixed with the domain (`compact:toggle`, `toolbar:hover`, `media:toggle`, `media:skip`, `drag:begin`, `drag:end`, `drag:drop`, `split:layout`, `split:sizes`, `space:profile`, `route:add`, `route:remove`, `library:open`, `history:query`, `history:delete`, `archive:list`, `archive:restore`, `archive:delete`, `tab:rename`, `tab:icon`, `site:panel`, `site:permission`, `site:clear`); pushed events `library:section`, `tab:rename-request`, `tab:icon-request`.

File split (to limit conflicts):

- Batch A: app/desktop/src/main/browser/compact.ts, app/desktop/src/main/browser/media.ts, app/desktop/src/renderer/src/ui/MediaCard.tsx, route `#toolbar`.
- Batch B: packages/shared/src/split.ts (rectangle computation, dividers, insertion), app/desktop/src/renderer/src/tabs/DropZones.tsx, app/desktop/src/renderer/src/tabs/Dividers.tsx.
- Batch C: app/desktop/src/main/web/profiles.ts (sessions), app/desktop/src/main/web/permissions.ts, packages/shared/src/routes.ts, app/desktop/src/renderer/src/lib/useSpaceSwipe.ts, "Routing" section of Settings.
- Batch D: app/desktop/src/main/windows/library.ts, app/desktop/src/main/browser/sitepanel.ts, app/desktop/src/renderer/src/windows/Library.tsx, app/desktop/src/renderer/src/windows/SitePanel.tsx, route `#library`.
  Shared files (at the time: browser.ts, tabs.ts, index.ts, preload, store.ts, palette.ts, menu.ts, Sidebar.tsx, TabRow.tsx, App.tsx, Toolbar.tsx, Settings.tsx, styles.css; today most of browser.ts is split across `main/browser/`): targeted Edits only.

### Batch A: Compact mode, media, picture in picture

**Compact mode** (`settings.compactMode`). Toggle: View menu "Compact Mode" ⌥⌘S, command bar action `compact`, switch in Settings › General. ⌘S in compact mode leaves compact mode and shows the sidebar.

- Layout: sidebar and toolbar hidden, traffic lights hidden; the web card fills `{ x: 6, y: 6, width: W - 12, height: H - 12 }`.
- Left edge: the existing floating sidebar (unchanged).
- Top edge: a 6 px strip at the top of the window (in the main renderer, above the card) calls `setToolbarHover(true)`. Main then shows a `#toolbar` view (same `Toolbar` component, almost opaque `--surface` background, edge, shadow) at `{ x: card.x + 8, y: 6, width: card.width - 16, height: 40 }`, radius 10, and shows the traffic lights again. `setToolbarHover(false)` (the view's mouseleave) hides it after 250 ms, cancellable. Stacking order: above the tabs, below the overlay.
- Transitions: the web card changes size at once (native views); the floating toolbar appears with a 6 px translation + fade (--t) and leaves the same way; the floating sidebar slides in from the left edge (`translateX(-100%)` → 0 within its view's bounds) and goes back there. Main keeps the view in place during the exit (200 ms, 120 ms with reduced motion); the pointer coming back cancels it and starts from the current position (CSS transitions).

**Media** (`Snapshot.media`).

- Tracking per tab: `media-started-playing`, `media-paused`, `audio-state-changed`. A tab is a candidate if it plays, or if it played and has been paused for less than 10 minutes. `media` = the most recently active candidate **that isn't shown**; `null` otherwise.
- Metadata: on each state change, `executeJavaScriptInIsolatedWorld(1002, …)` reads `navigator.mediaSession.metadata` (title, artist, last artwork); failing that, the tab title. `canSkip` = MediaSession metadata exists.
- `mediaToggle`: `executeJavaScript(code, true)` (user gesture) that pauses the playing media, or resumes the last paused media. `mediaSkip`: `sendInputEvent` of the `MediaNextTrack` / `MediaPreviousTrack` keys (keyDown then keyUp), best effort.
- `MediaCard` interface: a compact card right above the sidebar's bottom bar: 32 px artwork (otherwise favicon), title and artist on two truncated lines, buttons previous (if `canSkip`), play/pause, next (if `canSkip`), mute. Click on the card → `activateTab`. Appears by sliding 8 px up + fade.

**Automatic picture in picture** (`settings.autoPip`, switch in Settings › General). When a tab leaves the display while one of its videos is playing: BEFORE removing its view, `executeJavaScript` with a user gesture that calls `requestPictureInPicture()` on the playing video (if none is already in PiP). When the tab comes back to the display: `document.exitPictureInPicture()` if its video is in PiP. Errors ignored.

### Batch B: Advanced split view

- Up to **4** tabs per split view (instead of 3). Layouts: `columns` (side by side), `rows` (stacked), `grid` (two rows: `ceil(n/2)` on top, the rest below; only for n ≥ 3, otherwise same as `columns`).
- Rectangles (split.ts, pure testable function): from the content area, `layout`, `sizes` and `GAP = 8`. Minimum pane size: 200 px (clamp, redistributing). `Snapshot.dividers`: in `columns`, `n - 1` vertical dividers (8 px strip between two panes, `index` = boundary between part `index` and `index + 1`); in `rows`, horizontal; in `grid`, a single horizontal divider between the rows (`index` 0); the columns of a row stay equal.
- `Dividers` interface (main window, below the views): each divider is an 8 px grab area (`cursor: col-resize` / `row-resize`); on hover, a rounded 3×28 px handle appears in the middle; while dragging, it takes the accent. Dragging = 1:1 tracking of the pointer (Pointer Events + `setPointerCapture`, respecting the grab offset), `setSplitSizes` sent at most once per frame (`requestAnimationFrame`). Double-click = equal parts.
- **Dragging a tab onto the page**: on `dragstart` of a sidebar tab row (a single tab, not a folder or a whole split view; a favorite can only do `center`), the interface calls `beginTabDrag(tabId)`. Main opens the overlay in `dropzones` mode (`area` = rectangle of the web card). `DropZones` receives the HTML5 drag events (same MIME type as the sidebar): 25 % strips on each edge → `left` / `right` / `top` / `bottom`, center → `center`. While hovering, a translucent preview (`--accent` at 18 %, accent edge, radius 10) shows the place the tab will take, and it slides to the new zone with a 140 ms transition. Drop → `dropOnContent(tabId, zone)`. `dragend` in the sidebar or Esc → `endTabDrag()` closes the overlay.
- Drop semantics: `center` → activates the tab. Edges: the tab joins the split view of the shown tab (creating it if needed), inserted before (left/top) or after (right/bottom) the focused pane. `left`/`right` → `columns` layout (if it was `rows` and n ≥ 3 after adding → `grid`); `top`/`bottom` → `rows` (if it was `columns` and n ≥ 3 → `grid`). Parts are reset to equal. If the split view already has 4 tabs → like `center`. If the shown tab is a favorite → like `center`.
- Changing layout: split view context menu in the sidebar "Layout ▸ Side by Side / Stacked / Grid" (Grid only if n ≥ 3), command bar actions `split-columns`, `split-rows`, `split-grid`, `split-equalize` "Equalize Panes". The sidebar's split view row shows a small layout icon (`Columns2` / `Rows2` / `Grid2x2`).
- The find bar and Peek follow the new rectangles.

### Batch C: Advanced spaces

**Profiles** (`Space.profile`).

- `isolated` → partition `persist:space-<spaceId>`; `shared` (default) → default session. Favorites, Peek opened from a favorite and Little Signet (outside routing) use the default session; a tab uses its space's session; Peek uses the session of the tab it came from.
- profiles.ts: `sessionFor(spaceId | null)` and `configureSession(ses)`, called once per session: user agent, permission handler (permissions.ts), ad blocker (adblock.ts must handle several sessions), downloads (downloads.ts must listen to `will-download` on each session). Replace direct uses of `session.defaultSession` with these functions.
- Changing a space's profile (`setSpaceProfile`): confirmation dialog ("The tabs of this space will reload. Signed-in accounts will not be shared with the other spaces.") then destroy the views of its tabs (reloaded when shown). Moving a tab to a space with a different profile destroys its view the same way.
- Space form: "Isolated profile" switch with a sentence of explanation. Space menu: "Isolated Profile" checked or not, and "Clear Space Data…" (isolated profile only: `clearStorageData` + `clearCache` of its partition, confirmation). Sidebar header: a small `UserRound` icon next to the name of isolated spaces (with an explanatory `title`).

**Persistent permissions** (permissions.ts, also used by batch D). `Permissions` class on `store.state.sitePermissions: Record<origin, Record<permission, 'allow' | 'deny'>>`: `get(origin, permission)`, `set(origin, permission, state: PermissionState)` (`ask` = delete the decision), `list(origin)`. The request handler checks these decisions before showing the dialog and records the answer there (instead of in session memory). Also add `setPermissionCheckHandler`, which answers according to these decisions. The ad blocker counts blocked requests per `webContents` (the library's `request-blocked` event), reset on each main frame navigation: method `blockedCount(webContentsId)` in adblock.ts.

**Routing** (`settings.routes`, routes.ts). `matchRoute(url)`: host equal to the pattern or a subdomain of it; `*` in the pattern = any run of characters except `/`. Applies to new tabs opened from the command bar (`new-tab` mode) and to external links (`open-url`, second launch): if a rule points to another space, the tab opens in that space (which becomes active) in the main window, instead of the active space or a Little Signet. Settings: new "Routing" section (`Signpost` icon): list of rules (pattern → emoji + space name, delete button), add form (pattern field + space picker). Tab context menu: "Always open <site> in ▸" (submenu of the spaces).

**Swiping between spaces** (useSpaceSwipe.ts, in the sidebar and the floating sidebar):

- `wheel` events with `|deltaX| > |deltaY|` on the sidebar (outside text fields). 1:1 tracking: the track of space lists (previous, current, next, each as wide as the sidebar; favorites stay fixed) follows the accumulated `deltaX`. Hard stop at the first and last space: pushing outward moves nothing (no elastic, no bounce back).
- End of gesture: no event for 90 ms. Velocity = average of the last moves (px/s). Projection: `position + (v / 1000) * 0.998 / (1 - 0.998)`. If the projection goes past half the width → spring animation (damping 1, response 0.35 s, initial velocity = gesture velocity) to the neighbor space, then `switchSpace`; otherwise spring back. A small `spring.ts` utility in the renderer (critical spring integrated with `requestAnimationFrame`, interruptible: a new gesture starts from the displayed position).
- `prefers-reduced-motion`: no tracking, switch at the threshold with a 150 ms fade.
- The highlight of the space emojis at the bottom (background and opacity) follows the gesture's progress.
- BrowserWindow `swipe` (three-finger gesture, if enabled in macOS) → previous / next space.

### Batch D: Library, renaming tabs, site panel

**Library**: a single `#library` window (like Settings: 920×660, navigation on the left in vibrancy, content on the right). Opening: ⌘Y, action `library` "Library", `Library` button in the sidebar's bottom bar (next to the gear), `openLibrary(section)` (pushes `library:section` if already open).

- History: search (accent insensitive), list grouped by day ("Today", "Yesterday", then "Tuesday, October 21"), row = favicon, title, domain, time; click → new tab in the main window; ✕ on hover → `deleteHistory`; loading in pages of 100 while scrolling; "Clear history…" button (confirmation, `clearBrowsingData(['history'])`).
- Archive: search, grouped by day, "Restore" (Today tab in the active space, removed from the archive) and ✕.
- Downloads: full list (same rows as the sidebar panel), Clear.
- Spaces: one card per space (emoji, name, hue swatch, number of pinned and Today tabs, isolated profile badge); click → `switchSpace` and main window to the front; "Edit" button → space form in the sidebar (`space:edit`).

**Renaming a tab / giving it an emoji**: double-click on the title of a tab row → inline field (Enter confirms, Esc cancels, empty = back to the page title) → `renameTab`. Tab context menu: "Rename…" (`tab:rename-request`), "Choose an Icon…" (`tab:icon-request` → the sidebar opens the existing `EmojiPicker` in a panel, like the space form → `setTabIcon`), "Reset Name and Icon" if customized. Display: `customIcon` in place of the favicon (15 px emoji), `customTitle` in place of the title in the sidebar, the command bar and favorite tiles (`title`). The toolbar keeps the real page title.

**Site panel**: click on the toolbar's padlock (or the info icon for http) → `openSitePanel(anchor)` → main computes the active tab's `SiteInfo` and opens the overlay in `site` mode. `SitePanel`: a 320 px card below the anchor (discreet arrow), no scrim, but a click outside or Esc closes it. Content: header (favicon, site, "Secure connection" / "Connection not secure"); permissions Camera, Microphone, Location, Notifications, Clipboard with an Ask / Allow / Block segmented control (`setSitePermission`, through batch C's `Permissions` class: `media` is stored separately for `camera` and `microphone` if possible, otherwise a single "Camera and microphone" row); "N items blocked on this page" (through batch C's `blockedCount`); Boost row (switch + "Edit"); footer "Clear site cookies and data" (`clearStorageData({ origin })` on the tab's session, then reload, inline confirmation).
**Boost indicator**: `Snapshot.boostedSites`; the toolbar rocket takes `--accent` when the shown site is in it.

### Interaction principles (for all batches)

Taken from Apple's guidelines for fluid interfaces:

- Visual feedback on `pointerdown`, never only on click: rows, tiles and buttons press in slightly (`scale(.985)`, 80 ms).
- Anything the user drags follows the pointer 1:1, respecting the grab point, and stays interruptible.
- Short animations without bounce by default (critical spring); a slight bounce only after a gesture with momentum.
- Enter and exit by the same path; panels grow from the element that opens them (`transform-origin`).
- `prefers-reduced-motion`: short fades instead of translations.
- Elastic formula: `rubberband(d, dim, c = 0.55) = (d * dim * c) / (dim + c * |d|)`.

## v6: design polish

Goal: go from a "clean" interface to a refined, calm and lively one. v4 set the sobriety (no gradients); this pass works on geometry, material, typography and motion. It applies to everything: main window, floating bars, command bar, Peek, site panel, drop zones, Little Signet, Boost, Settings, Library. The behavior doesn't change.

### Geometry

- **Concentric radii**: a nested element has its parent's radius minus the spacing between them. Tokens: `--r-window: 16px` (macOS window corner), `--gap: 8px` (spacing between the sidebar, the toolbar and the web card), `--r-card: calc(var(--r-window) - var(--gap) / 2)` = 12 px for the web card (so `setBorderRadius(12)` on the main side for tab and Peek views), `--r-row: 8px` for sidebar rows, `--r-tile: 10px` for favorite tiles and folder cards, `--r-control: 7px` for buttons and fields, `--r-pop: 14px` for the command bar, the site panel and floating panels.
- **Superellipse corners** on everything drawn in CSS: `corner-shape: superellipse(1.3)` (Electron 45's Chromium supports it; no effect elsewhere). Not on native views.
- **4 px grid**: all spacings, heights and margins are multiples of 4 (justified exceptions: 1 px lines, the shared `--gap`). Tab row 32 px, favorite tile 44 px, toolbar 44 px, bottom bar 36 px.
- Alignment: the sidebar icons (favicons, folder icons, "+ New tab") are all on the same column; the titles on the same starting line.

### Material and depth

- **The web card floats**: a soft shadow drawn by the renderer behind each pane (an element exactly under the native view): `0 1px 2px rgb(0 0 0 / .10), 0 3px 8px rgb(0 0 0 / .18)` in dark mode, `0 1px 2px rgb(0 0 0 / .06), 0 3px 8px rgb(0 0 0 / .10)` in light mode. A 0.5 px inner `--border` edge around the card.
- **Slightly tinted neutrals** (no gradient): `--bg`, `--surface`, `--surface-2` and `--border` get 2 to 4 % of the space hue through `color-mix(in oklch, …)`. Space change: 240 ms color transition.
- **Floating layers** (command bar, site panel, floating bars, interface menus): `backdrop-filter: blur(24px) saturate(160%)`, `--surface` background at 82 % (dark) or 78 % (light), lighter top edge (`box-shadow: inset 0 0.5px 0 rgb(255 255 255 / .12)` in dark mode), stronger shadow than the web card since it sits higher: `0 12px 32px rgb(0 0 0 / .28), 0 2px 6px rgb(0 0 0 / .16)`. Never stack two light translucent layers.
- **Scrim** only for modal tasks (command bar: `rgb(0 0 0 / .18)`; Peek: `.45`). The site panel and drop zones have no scrim.
- **Scroll edge**: when the sidebar list scrolls under the header or above the bottom bar, a 12 px fade mask (`mask-image`) instead of a line.

### Typography

- System font (`-apple-system`, `system-ui`), `font-optical-sizing: auto`, smoothing `-webkit-font-smoothing: antialiased`.
- Scale: 11 px (captions, shortcuts, counters) `letter-spacing: .01em`; 12 px (subtitles); 13 px (body text, tab rows) weight 450; 13 px weight 600 (space name, sidebar section titles); 20 px weight 650 `letter-spacing: -0.01em` (Settings and Library titles); 28 px weight 700 `-0.02em` (About).
- Hierarchy through weight and color (`--text`, `--text-2`, `--text-3`) rather than size. Tabular figures (`font-variant-numeric: tabular-nums`) for sizes, times and counters.
- Tab titles: `--text` for the active tab, `--text-2` for the others (a little more readable on hover).

### Motion

- Animation tokens in `:root`: `--ease-out: cubic-bezier(.2, .8, .2, 1)`; critical spring in CSS through `linear()`: `--spring: linear(0, .17 4%, .48 10%, .72 17%, .87 25%, .95 34%, .985 45%, 1)` (response ≈ 0.35 s, no bounce); durations `--t-fast: 120ms`, `--t: 200ms`, `--t-slow: 320ms`, `--t-hud: 150ms` (HUD / quit / space change fades); weights `--fw-body` 450, `--fw-medium` 500, `--fw-label` 550, `--fw-strong` 600, `--fw-title` 650, `--fw-hero` 700; material for large modal surfaces (command bar): `--blur-lg: blur(40px) saturate(170%)` and `--shadow-modal` (deeper than `--shadow-pop`). `prefers-contrast: more` and `prefers-reduced-transparency` make `--glass`, `--glass-view` opaque and neutralize `--blur` / `--blur-lg` (contrast: `--edge-light` too).
- **Touch feedback**: `:active` → `scale(.985)` in 80 ms on rows, tiles, buttons, space emojis; hover in `--t-fast`.
- **Appearances**: the command bar and the site panel grow from their origin (bar: slight `scale(.98)` + `translateY(-4px)` + fade; site panel: `transform-origin` on the padlock) with `--spring`, and leave the same way. Folders open by animating the height (`grid-template-rows: 0fr → 1fr`) and the chevron rotation.
- **Active tab change**: the active tab's highlight slides to its new position when it is in the same list (a single absolutely positioned element that follows the active row, with `--spring`); otherwise a simple fade.
- **Download**: when a download starts, a small chip (favicon or file icon) leaves the center of the web card and follows an arc to the downloads button (`offset-path` or two composed transforms), 600 ms, then the button does a slight bounce (the only bounce in the app, justified by an object arriving). The event comes from main (`downloads` growing).
- **Adding and removing tabs**: the row appears by unfolding its height from 0 to 32 px with a fade (`--t`), and disappears the same way.
- `prefers-reduced-motion: reduce`: 120 ms fades instead of translations, scales and arcs; no animation of the active highlight. `prefers-reduced-transparency: reduce`: opaque floating layers, no blur. `prefers-contrast: more`: `--border` at 20 %, darker secondary text.

### Details to get right

- Visible and consistent keyboard focus: 2 px `--accent` ring offset by 2 px (`outline-offset`), only in keyboard navigation (`:focus-visible`).
- Native tooltips (`title`) on all icon buttons, with the shortcut in parentheses when there is one.
- Thin, discreet scrollbar in the sidebar (6 px wide, visible only while scrolling).
- Polished empty states (Library, Boosts, downloads, search with no results): a 28 px `--text-3` icon, one sentence, an action if useful.
- No hard-coded color outside the tokens; no spacing value off the 4 px grid without a reason.

## v7: toolbar blended into the site

The toolbar is no longer a strip above the page, on the window background: it is part of each pane's **card** and takes **the color of the top of the site**, so the bar and the page form a single surface. Contract: `TabInfo.themeColor`, `Pane.card`, `LittleState.themeColor`.

### Layout

- The content column no longer reserves a 44 px strip at the top of the window. Each pane is a card: `card` = its rectangle in the content area (which starts at `y = 8`, like the sidebar, which keeps its 44 px header with the traffic lights); `rect` (the web view) = `card` minus the height of the pane bar (`PANE_BAR = 40`) at the top.
- The split view rectangle computation (split.ts) works on the cards; the dividers stay between the cards. Drop zones cover the cards. The find bar and Peek are placed relative to `rect`.
- Web views: `setBorderRadius(12)` unchanged. Their rounded top corners sit on the pane bar, of the same color: the seam is invisible.
- Sidebar hidden: the leftmost card reserves 78 px on the left of its bar for the traffic lights.
- Compact mode: the card fills the whole window minus 6 px, **without** a pane bar (web view = card); the existing floating toolbar stays the way to navigate, and it takes the site color too.

### Pane bar (main window renderer, drawn in `card` above the view)

- Background = `themeColor` of the pane's tab (200 ms color transition, no animation if `prefers-reduced-motion`); failing that, `--surface`. Top corners of the card rounded to 12, with the same card shadow as before (the shadow surrounds the whole card, bar included).
- Content color computed by contrast: white contrasts better than black on `themeColor` (WCAG ratios) → light text and icons (`rgb(255 255 255 / .92)`, secondary `/ .55`); otherwise dark (`rgb(0 0 0 / .85)`, secondary `/ .5`). Button hover: the content color at 10 %.
- Content, from left to right: Back, Forward, Reload; then, with no pill or background, the padlock (or the info icon) followed by "**domain** / title" (title in the secondary color, truncated): click on the padlock → site panel, click on the text → edit the address; on the right the Boost rocket (accent if on).
- The whole bar is a `drag` area (the window moves when you grab it), the buttons and clickable text in `no-drag`.
- Split view: each pane has its own bar, which acts on its tab (the buttons target `pane.tabId`). The focused pane keeps its ring; an unfocused pane's bar shows its content at 70 % opacity. If the card is narrower than 420 px: only Reload, the padlock and the domain.
- No more global toolbar above the panes (Toolbar.tsx becomes a pane's bar, also reused by the compact mode floating toolbar).

### Site color (main)

- For each shown tab: sample the top of the page with `webContents.capturePage({ x: 0, y: 0, width, height: 2 })` (width of the view), then take the most frequent color of the first row's pixels (quantized to 4 bits per channel to ignore anti-aliasing), ignoring fully transparent pixels. Format `#rrggbb`.
- When: `did-finish-load`, `did-navigate-in-page`, `dom-ready` + 300 ms, when the tab becomes shown, then every 1.5 s while it is shown and the window is visible (the color can change on scroll or on a light/dark switch). Only update `themeColor` (and so push the state) if the color changed.
- If sampling fails or the page is empty: use the last `did-change-theme-color` color (`theme-color` meta) if there is one, otherwise leave `themeColor` undefined.
- Little Signet: `LittleState.themeColor`; its 44 px bar takes the site color with the same contrast computation. Compact mode floating toolbar: same.

### Check

`SIGNET_SEED=1` + captures: the real page can't be seen in a capture of the window (native views); to check the blend, the capture hook must, in capture mode only, compose the window image with the web views' `capturePage()` at the `rect` positions. Check at least: a light page (wikipedia.org), a dark page (a site with a black background), a split view with two different colors, the sidebar hidden (traffic lights on the bar), in light and dark mode.

## v8: website languages and extensions

Project licence: GPL-3.0-or-later (LICENSE file). Contract: `Settings.languages`, `ExtensionInfo`, `SettingsInfo.extensions`, `openExtensionStore`, `removeExtension`. Migration: if `settings.languages` is missing or empty, it is computed on load from the system languages (`app.getPreferredSystemLanguages()`, each regional code followed by its base language, without duplicates: `en-US, fr-FR` → `en-US, en, fr-FR, fr`); fallback `['en-US', 'en']` if the system gives none.

### Website languages

- Websites must receive the languages from `settings.languages` in the `Accept-Language` header and in `navigator.language` / `navigator.languages`.
- Header: for **each** session (default and isolated profiles, see profiles.ts `configureSession`), `ses.setUserAgent(userAgent, acceptLanguages)` with the plain list `fr-FR,fr,en-US,en` (Chromium adds the weights itself: `fr-FR,fr;q=0.9,en-US;q=0.8,en;q=0.7`). The default values follow the system, not a fixed language. Check that no other `setUserAgent` call overwrites the languages.
- `navigator.language` / `languages`: Chromium takes them from the system language and ignores `setUserAgent`; so each tab view gets `Emulation.setUserAgentOverride` (debugging protocol, `emulateLanguages`) with the same list, and with `userAgentMetadata`: Chromium's native hints, read once in the main interface view (not overridden) as soon as it loads (`captureNativeHints`: brands with the version-specific GREASE brand, e.g. "Not:A-Brand" v8 for 156, full version, platform and its version, architecture), then reapplied to the tabs already open. Never make them up: fake hints look like no real Chromium. Without this metadata, the override wipes the client hints (`Sec-CH-UA`, empty `navigator.userAgentData.brands`): a "Chrome" without hints passes for an embedded browser and Google refuses the sign-in. The Develop menu keeps the default identity as long as no agent is chosen. Google sign-in: refused anyway ("This browser or app may not be secure"), including when announcing itself as Firefox on accounts.google.com (tried then removed: Google spots Chromium in the page and, probably, through the TLS fingerprint of the connection). Comparison with plain CEF 156 (accepted by Google, Rust cef-rs `cefsimple` prototype): gaps found on the Electron side: full version in the agent (fixed), hints made up by Signet (fixed), no client hints on navigations nor detailed hints via `Accept-CH`, empty `window.chrome` (CEF: `app`, `csi`, `loadTimes`), notifications "granted" by default. In addition, `app.commandLine.appendSwitch('lang', languages[0])` before `ready` (read from state.json) sets the application language; a change of the first language fully applies at the next launch (say so in the interface).
- List change: reapply the header to all sessions right away, then reload the tabs on screen.
- Settings › General, "Website languages" group: ordered list (drag or up/down arrows), delete button per language (at least one), add through a picker of common languages (name in its own language: Français, English, Español, Deutsch, Italiano, Português, Nederlands, 日本語, 한국어, 中文…, plus the useful regional variants fr-FR, fr-CA, fr-BE, en-US, en-GB…), "Back to system languages" button (sends an empty list, which the main process replaces with the system languages), note "The primary language fully applies at next launch."

### Extensions

Libraries (already installed, in `dependencies`): `electron-chrome-extensions` (extension API: tabs, windows, action buttons, context menus…) and `electron-chrome-web-store` (install and updates from the store). All the code that uses them lives in `app/desktop/src/main/features/extensions/extensions.ts` (and the preload for the action buttons): this is the approved exception to the "no other browser name" rule; the interface labels stay neutral ("Extensions", "Extension store"). Read their README and their types in node_modules before coding (if a permission rule blocks reading them, use their public repository github.com/samuelmaddock/electron-browser-shell via `gh`).

- Extensions stored in `<userData>/Extensions`; loaded in **each** session (default and isolated profiles) at startup and when a session is created.
- One `ElectronChromeExtensions` instance per session, with the required callbacks: create a tab (new Today tab in the active space, or in the session's space), select a tab, close a tab, create / close a window (Little Signet for popup windows, otherwise a tab). Each tab view is declared to the library when it is created (`addTab`) and when it is activated (`selectTab`).
- Store: `installChromeWebStore` for each session, so that the store's "Add" button works in Signet and the banner asking for another browser disappears. Automatic updates enabled.
- Action buttons: in the bar of each pane, on the right, just before the Boost rocket, the buttons of the extensions that have one (element provided by the library, injected by the main window's preload; adapt the electron-vite configuration so that the preload bundles this code, because a `sandbox` preload cannot `require` a module). They follow the bar's content color (`--bar-fg`) as much as possible. Extension popups open below their button.
- Context menus: the entries added by extensions appear in the page context menu (tabs.ts).
- Settings: new "Extensions" section (`Puzzle` icon): list of installed extensions (icon, name, version, "Remove" button with in-place confirmation), "Open the extension store" button (`openExtensionStore` → new tab on the store in the main window), and an honest note: "Most extensions work. Some that need advanced features (network, proxy, identity) may not."
- The built-in ad blocker stays independent.

## v9: interface in English and French

The whole Signet interface becomes translatable. Languages available at first: English and French. By default, the interface follows the macOS language (`app.getPreferredSystemLanguages()`: the first supported language, otherwise English). The user can force a language in Settings. No more visible text hard-coded in the code.

### Mechanics (already in place)

- Libraries: `i18next` (dependencies, used by main and the interface) and `react-i18next` (devDependencies, bundled into the interface by Vite).
- packages/shared/src/i18n.ts: `createI18n(locale, defaultNS)` creates an independent i18next instance, `resolveLocale(pref, systemLanguages)`, `LOCALES`, `resources`. app/desktop/src/shared/i18next.d.ts types the keys (TypeScript flags an unknown key).
- Standard JSON catalogs, editable by contributors or translation tools: `packages/shared/src/locales/<en|fr>/main.json` (main process: application menu, native context menus, dialogs, command bar actions and subtitles, import summaries, window titles) and `ui.json` (React interface). English is the reference; a key missing in French falls back to English, but French must be complete (checked by the check script).
- i18next syntax: nested `zone.element` keys in English (`sidebar.newTab`, `settings.general.title`, `palette.action.pin`, `dialog.deleteSpace.message`), `{{name}}` interpolation, plurals through the `_one` / `_other` suffixes with `count`. No generic key reused for different meanings.

### Contract

- `Settings.uiLanguage?: UiLanguage` (missing = `system`).
- `SignetApi.getLocale(): Promise<Locale>` and `SignetApi.onLocale(cb)`: all views (main window, overlay, floating bars, search, Little Signet, Boost, Settings, Library) read the language on mount and update live when it changes.

### Main

- `app/desktop/src/main/app/i18n.ts`: current language (preference + system languages), `t()`, language change → rebuild the application menu, push `locale` to all the interface `webContents`, recompute what depends on it (command bar subtitles on the next call).
- All visible texts in main go through `t()`: application menu (including the labels of the Edit and Window roles), context menus of pages, tabs, spaces, folders, split views, dialogs (permissions, confirmations, import, profiles), command bar actions and subtitles, window titles.
- The name of the subfolders created by the import ("Other Bookmarks"…) follows the language at the time of the import.

### Interface

- `I18nextProvider` (react-i18next) at the root of each route (main.tsx), with an instance created by `createI18n(locale, 'ui')`; `useTranslation()` in the components; language change → `i18n.changeLanguage()`.
- All visible texts (labels, `title` tooltips, `aria-label`, alt texts, empty states, confirmations) go through `t()`.
- Dates, times, sizes and numbers: `Intl` with the current language (Library, downloads, Boosts). Day names and "Today / Yesterday" come from the catalog or from `Intl.RelativeTimeFormat`.
- Emoji picker: emojibase data of the current language (`emojibase-data/en/…` or `fr/…`), search in that language.
- Settings › General, at the top: "Interface language": "System (English)" / "English" / "Français"; the change applies immediately everywhere.
- The shortcuts table in Settings is translated too.

### Checks

- Check script (in app/desktop/scripts/, run by `npm run i18n:check`) that checks that fr and en have exactly the same keys and that fails if a hard-coded visible string remains in `app/desktop/src/renderer/src/**` and in the main labels (heuristic: JSX text strings and `title`, `aria-label`, `placeholder`, `label:` attributes containing letters, excluding keys and proper names listed explicitly such as "Signet", "Little Signet", "Peek", "Boost"). Run it and keep it clean.
- Screenshots (dev hook, isolated profile) of the main window, of the command bar if possible, of Settings and of the Library, in English and in French.

## v10: swipe to navigate, extension menu

Contract: `Settings.swipeNavigation?` (missing = enabled), `SignetApi.showExtensionMenu(id)`.

### Two-finger swipe to go back or forward

Electron's engine does not include trackpad swipe navigation: we rebuild it.

- **Page script** (app/desktop/src/preload/swipe.ts, a separate preload entry built by electron-vite into a single standalone file, `sandbox` compatible) registered on each session with `ses.registerPreloadScript({ type: 'frame', … })` (profiles.ts `configureSession`); it only acts in the main frame of tabs (not in the interface views, nor in extension popups).
- Detection, on `wheel` events (passive listener, in the capture phase on `window`): horizontal gesture (`|deltaX| > 1.5 × |deltaY|` on the first events, then locked for the whole gesture) AND no element under the pointer (nor the page) can still scroll horizontally in that direction (walk up the ancestors: `overflow-x` auto/scroll and `scrollLeft` not at its limit) AND the event was not cancelled by the page (`defaultPrevented` checked in a listener added after the page's, e.g. via `setTimeout(0)` or as a non-capturing listener on `window`). End of the gesture: no event for 150 ms (trackpad momentum events extend the gesture), but we don't wait for it to decide: pages don't have the gesture phase, so the release is detected through momentum (decreasing |deltaX|, 3 decreasing events with the last ≤ 0.9 × the first after the threshold, 4 before the threshold). Threshold crossed + momentum detected (or movement ≥ 2.5 × threshold) → immediate navigation; momentum detected before the threshold → the bubble goes back right away. Then the rest of the stream is ignored until 150 ms of silence, except a clear rise of |deltaX| (3 events), which signals new fingers. A stream that starts already decreasing (momentum arriving on the reloaded page) does not open a gesture. Main pause after a swipe navigation: 400 ms. Measured (SIGNET_SWIPECHECK, fingers + 1 s of inertia): last finger → navigation ≈ 1.2 s before, ≈ 100-140 ms after (including ≈ 25 ms until the navigation starts).
- Direction: swipe right (content pulled to the right, negative `deltaX`) = back; left = forward. At the start of a gesture, the script asks main (`ipcRenderer.invoke('swipe:can')`) whether the tab can go back / forward; with no history in that direction, it shows resistance (the bubble barely moves, elastic formula) and does not navigate.
- **Indicator** (drawn by the script in the page, in a closed `shadow root` so that no page style affects it, `position: fixed`, `pointer-events: none`, maximum z-index): a round 40 px bubble with an arrow, which comes in from the left edge (back) or the right edge (forward) and follows the finger 1:1 up to the threshold (120 px of cumulative movement), with a slight resistance beyond it; at the threshold, the bubble fills with the accent and grows slightly (visual feedback "this will navigate"). On release: beyond the threshold → `ipcRenderer.send('swipe:navigate', 'back' | 'forward')` and the bubble fades out outward; otherwise it springs back. Respects `prefers-reduced-motion` (fade).
- Main: `swipe:can` and `swipe:navigate` resolve the tab from `event.sender`; navigation through `navigationHistory`. Ignore if `settings.swipeNavigation === false` (the script gets the setting through `swipe:can`).
- Settings › General: "Swipe with two fingers to navigate" switch (translated in en and fr).
- Do not break: horizontal scrolling of pages (carousels, wide tables, maps), swiping between spaces on the sidebar (another view), the system's three-finger gestures.

### Pinned extensions and Extensions button

- Only pinned extensions (`settings.pinnedExtensions`) have a button in the toolbar, in the `extensionOrder` order (drag and drop). An installed, imported or restored extension is never pinned by default; on the update that introduced pinning (replacing `hiddenExtensions`, "everything shown except the hidden ones"), all extensions are unpinned.
- Extensions button (puzzle piece, after the pinned buttons, present as soon as an extension has a button) → `showExtensionsMenu()` → native menu: each extension with a button (16 px icon, name; click = its popup opens below the Extensions button, the activation happens on the interface side with that button's rectangle), separator, "Pinned to Toolbar" submenu (checkboxes), "Manage Extensions…".
- Settings › Extensions: the switch of each extension with a button pins or unpins it.

### Extension button menu

- Right click on an extension button in the bar → `showExtensionMenu(id)` → translated native menu: name of the extension (grayed out), "Options" (if the extension declares an options page: open it in a new tab), "Unpin from Toolbar", "Manage extensions" (Settings › Extensions), separator, "Remove from Signet…" (native confirmation dialog then removal from all sessions, like `removeExtension`). If the library already provides a context menu for its buttons, replace it with this one.
- Get the id of the extension under the pointer from the `browser-action-list` element (read how the library renders its buttons: attributes, `::part` parts, elements in the shadow DOM); if this is not possible cleanly on the interface side, go through the library's API on the main side.

## v11: internal `signet://` addresses

An internal address scheme, like those of other browsers. All the code lives in `app/desktop/src/main/web/internal.ts` (the only file, along with the list of the engine's technical pages, allowed to contain the engine's internal addresses: a naming exception, like the importers).

### Three families of addresses

1. **Shortcuts to Signet's screens** (nothing opens in a tab; if they are typed in the command bar in new tab mode, no empty tab is created):
   - `signet://settings` → Settings; `signet://settings/<section>` → the section (`general`, `appearance`, `privacy`, `import`, `boosts`, `routing`, `extensions`, `shortcuts`, `about`);
   - `signet://extensions` = `signet://settings/extensions`; `signet://about` = `signet://settings/about`;
   - `signet://library` → Library; `signet://library/<history|archive|downloads|spaces>`; `signet://history`, `signet://downloads` = the matching sections.
   - Unknown section → the window opens on its first section.
2. **Information pages served in a tab** by `protocol.handle('signet', …)` on each session (scheme registered before `ready` with `protocol.registerSchemesAsPrivileged`: `standard: true, secure: true`, without any special access): generated static HTML, translated with main's `t()`, cleanly laid out (the app's light / dark color tokens, system font, strict `Content-Security-Policy` without script):
   - `signet://urls`: the list of all available `signet://` addresses, clickable, grouped by family, each with a one-sentence description;
   - `signet://version`: version of Signet, Electron, the engine, Node, V8, system, interface language and website languages, user agent, profile folder, extensions folder;
   - unknown `signet://` address → "Address not found" page that links to `signet://urls`.
3. **The engine's technical pages**, exposed again under `signet://<name>`: check one by one which ones actually load in Electron 45 (candidates: gpu, media-internals, webrtc-internals, process-internals, accessibility, serviceworker-internals, indexeddb-internals, quota-internals, histograms, net-export, tracing, ukm, private-aggregation-internals, attribution-internals, topics-internals) and keep only those that work. `signet://gpu` loads the matching engine page in the tab.

### Display and input

- An engine technical page shows **everywhere** under its `signet://` address (toolbar, sidebar, history, command bar, Library): a `displayUrl(url)` function in main does the conversion for display and a `resolveInternal(url)` function the reverse conversion for navigation. The engine's real address must never appear in the interface.
- `toUrl()` recognizes `signet://…` (and `signet:…` without the slashes) as an address.
- Command bar: as soon as the input starts with `signet:`, suggest the matching internal addresses (title = address, subtitle = translated description) before everything else.
- `signet://` links clicked in a `signet://` page work (including the shortcuts to screens).
- Security: an ordinary web page cannot navigate to `signet://` nor to an engine technical page (refuse in `will-navigate` / `setWindowOpenHandler` if the origin the navigation starts from is not `signet:` or a user input).

## v12: tabs on top (⌃⇧S)

A second layout, to choose from: the sidebar (default) or a **row of tabs at the top of the window**, with the page below. Toggle with ⌃⇧S (View menu "Tabs on Top" / "Tabs on the Side", command bar action, Settings › Appearance). Contract: `Settings.tabLayout`. The behavior (spaces, favorites, pinned tabs, folders, split views, drag and drop, context menus, renaming, media, downloads) is the same; only the presentation changes.

### Layout (main)

- `horizontal` layout: no sidebar (not even floating); a 40 px **tab bar** at the top of the window (main window renderer, `drag` area), then the pane cards below it, with the usual margins (`x = 8`, `y = 40 + 4`). The traffic lights stay visible, vertically centered in the tab bar (`trafficLightPosition` adjusted to the layout).
- Compact mode + horizontal layout: the tab bar hides and shows again when hovering the top edge (like the floating toolbar, in the same floating view or a dedicated view); the cards take the whole window.
- Changing the layout reloads no page.

### Tab bar (renderer)

From left to right, after 78 px for the traffic lights:

1. **Space picker**: button (emoji + space name, truncated at 120 px); click → interface menu (not native) listing the spaces (emoji, name, check mark on the active one), "New space", "Edit Space…"; a space form opens in a popover below the button (the popover must go above the pages: use the existing overlay or a dedicated view, not the DOM of the main window, which is below the web views).
2. **Favorites** then **pinned tabs**: 28 px square chips with the favicon (or the custom emoji), without a title; tooltip with the title. A pinned folder = a folder icon chip (folder hue); click → dropdown menu listing its tabs (and subfolders as submenus), in the same layer above the pages.
3. Thin separator.
4. **Today tabs**: favicon + title chips, width between 120 and 220 px, which shrink when there are many of them and then scroll horizontally (wheel, with fades on the edges); close button on hover; the active tab in `--active`. A split view = a chip that groups the favicons of its tabs (and a layout icon), click → shows it.
5. **+** button (new tab ⌘T).
6. On the right: compact media card (if media is playing elsewhere: favicon/artwork + play/pause), downloads button (progress ring), Library, Settings.

Drag and drop: reorder within the row (2 px vertical insertion indicator), drop on a folder chip to put it there, on the space picker to send to a space (the menu opens on a long hover), and onto the page for the split view (existing drop zones). Same calls to main as the sidebar (`moveItem`, `beginTabDrag`…).

Right click, middle click, double click (rename): as in the sidebar. Two-finger swipe on the tab bar: horizontal scrolling of the row (no space change in this layout).

Style: same tokens as v6; tab bar on `--bg` (vibrancy), chips in `--surface-2` on hover, radius 8, height 30 px, short animations (a tab appears / disappears through its width), `prefers-reduced-motion` respected.

### Checks

Screenshots (dev hook, isolated profile, `SIGNET_SEED=1`) in horizontal layout, in light and dark: with favorites, pinned tabs including a folder, 6 Today tabs including the active one and a split view; then with 25 tabs (shrinking and scrolling); then with the space picker menu open and a folder menu open. Also check the ⌃⇧S round trip without a page reload.

## v13: floating mini player (custom picture in picture)

The picture in picture window built into the engine is basic and cannot be customized. We replace it with our own mini player, modeled on the modern picture in picture of browsers: a floating window with rounded corners, translucent controls on hover. Contract: `PipState`, `PipCommand`, `onPipState`, `pipCommand`, `openPip`. Code: app/desktop/src/main/windows/pip.ts, route `#pip` (app/desktop/src/renderer/src/windows/Pip.tsx).

### Principle

- The tab's **real page** is moved into a small window and only its video stays visible: the script (main world, `executeJavaScript`) marks the playing video, takes it out **synchronously** from its parent into a `position: fixed; inset: 0; z-index: 2147483647; background: #000` container added to the `body` (playback continues because the move happens in the same task), with `object-fit: contain`, `width/height: 100%`, and hides the scrollbars (`html, body { overflow: hidden }`). On return, the video is put back exactly in its original place (parent + next sibling remembered) and the injected style removed (`removeInsertedCSS`).
- **Window**: frameless `BrowserWindow` (`frame: false`, `roundedCorners: true`, `hasShadow: true`), `alwaysOnTop` at the `floating` level, `visibleOnAllWorkspaces` (including above full screen apps), `type: 'panel'` on macOS, not in the Dock nor in ⌘Tab, not resizable by the system (`resizable: false`, no `setAspectRatio`: with locked proportions, macOS only follows one axis and the edge drifts away from the pointer); resized by hand through invisible handles of the `#pip` view (4 corners of 14 px, 4 edges of 8 px), keeping the video proportions (`videoWidth / videoHeight`, 16:9 by default). Off-contract channel `pip:resize`: start / absolute pointer movement since the start of the gesture / end; main recomputes the geometry each time from the starting rectangle, so that the grabbed edge or corner stays under the pointer (corner: projection on the proportions diagonal, opposite corner fixed; edge: centered on the other axis). Valid width between 240 px and 90 % of the work area, but both bounds are elastic: a single path for the 8 handles (raw wanted width → `rubberband(overshoot, dimension, 0.55)` outside the bounds → height from the proportions → position from the anchor), so the window keeps following the pointer with growing resistance, and on release a critically damped spring (0.4 s response, started with the last resize velocity) brings it back to the closest valid size, anchor fixed; a new grab interrupts this return from the real rectangle. The screen edge stays a hard bound (the window never crosses it). Reduced motion: hard bounds, no animated return. No snapping nor saving during the gesture; the saved size is the final size, never the overshoot. Default size 400 px wide; default position at the bottom right of the main window's screen, 16 px from the edges; last size and position saved (store). The tab view takes the whole window with `setBorderRadius(12)`; a transparent `#pip` view is placed on top for the controls.
- **Drag** the window from anywhere outside the buttons (`drag` area of the `#pip` view); when released near a corner (less than 48 px), it snaps to the corner with a 16 px margin with a short animation (`setBounds` animated on the main side in small steps, or `animate: true`).

### Controls (`#pip` view, transparent background)

Invisible at rest; on hover, a `rgb(0 0 0 / .35)` veil appears in 150 ms with:

- top left: "Back to tab" button (diagonal arrow icon pointing up-left) in a translucent square chip (`rgb(255 255 255 / .16)` + blur, radius 10);
- top center: the site's domain (`www.` removed), white, 13 px, weight 600, one truncated line;
- top right: "× Close" chip (icon + text, translated);
- center: 15 s rewind (circular arrow icon with "15"), play / pause (large 40 px icon, rounded bars), 15 s forward; generous spacing; each button sinks slightly on click;
- bottom: thick progress bar (5 px, full radius, `rgb(255 255 255 / .35)` track, white progress), clickable and draggable to go forward or back, which grows slightly on hover; hidden for a live stream (infinite or zero `duration`); a sound button (mute / unmute) at the bottom right, discreet.
- Shortcuts when the window has focus: Space = play / pause, ← / → = ±15 s, M = sound, Esc = close.
- Everything goes through `pipCommand`; the state (`PipState`) is read again in the page every 500 ms while the mini player is open (playback, position, duration, sound) and pushed to the view.

### When it opens and closes

- **Automatically** (existing setting `settings.autoPip`): when leaving the display of a tab where a video is playing **with sound** (`heard`, see media.ts), instead of `requestPictureInPicture`. When the tab is shown again in the main window, the mini player closes and the page comes back into its card, video put back in place, without interrupting playback.
- **When leaving the window** (same setting): when the main window loses focus (another app, another desktop, another Signet window, minimizing) and a tab on screen is playing a video with sound (the active one first), mini player opened 250 ms later if the user has not come back (`away`). These do not count as leaving: windows placed over the main one (child, `chrome-extension://` extension popup), detached developer tools, the mini player itself. When focus comes back to the main window, this mini player (and only this one, not one opened by hand) closes and the video continues in its tab.
- **On demand**: "Picture in Picture" entry in the context menu of a page that contains a video, command bar action `pip`, and `openPip`.
- **Back to tab**: the window closes, the view comes back into the main window, the tab is activated (and its space), the main window comes to the front.
- **Close**: lets the video play, puts the page back in its tab (without showing it if it is not the active tab; the media stays controllable from the media card), closes the window.
- **Only a video that has a picture** (non-zero `videoWidth` and `videoHeight`): a music player that plays sound in a `<video>` without a picture (e.g. Tidal) never opens a mini player, neither automatically nor on demand, and the context menu does not offer the entry (`hasVideo`). Check: step (7) of `SIGNET_PIPCHECK`.
- Only one mini player at a time: opening another one closes the previous one (the previous video is paused).
- If the page navigates or the tab is closed: the mini player closes cleanly.
- The native picture in picture triggered by a site itself (player button) is not intercepted.

## v14: confirm before quitting (⌘Q)

To avoid closing the browser by accident. Contract: `Settings.confirmQuit?` (missing = enabled), `QuitHudState`, `onQuitState`. Code: app/desktop/src/main/app/quit.ts, route `#quit` (app/desktop/src/renderer/src/overlay/QuitHud.tsx).

- **Two ways to quit** when the setting is on:
  - **hold ⌘Q**: a bubble appears and its gauge fills in 900 ms; at the end, the app quits. Releasing before the end stops the gauge (it drops back in 150 ms) but leaves the bubble shown;
  - **press twice**: a second ⌘Q within 2 s of the first one (bubble shown) quits right away.
  - Without a new press, the bubble disappears 2 s after the release.
- **Bubble**: small frameless, transparent, always-on-top, non-focusable window (it does not steal focus), horizontally centered on the Signet window that has focus (or on the main screen if there is none), at the first third of its height. Content: rounded chip (radius 14, blurred translucent `--surface` background, floating layer shadow) with the text "Hold ⌘Q to quit" (and smaller "or press again"), translated en/fr, and a thin gauge at the bottom (accent) animated in the view (`requestAnimationFrame`) from `start`/`duration` sent only once by main (waiting cap at 0.4 as long as no key repeat has been seen, signaled by a single `repeating` message); on release it goes back toward zero from its current value. The 900 ms threshold is still decided by main. Appears with a fade + slight scale (150 ms), `prefers-reduced-motion` respected.
- **Detecting the held press**: the menu shortcut does not report releases. Listen to `before-input-event` on all the app's `webContents` (main window, tab views, overlay, floating bars, Little Signet, Settings, Library, Boost, mini player… via `app.on('web-contents-created')`): `keyDown` of Q with ⌘ alone (without Shift, Option or Ctrl) → `preventDefault`, start (or continue if `isAutoRepeat`); `keyUp` of Q or of ⌘ → released. The "Quit Signet" menu item keeps showing the ⌘Q shortcut but without registering the accelerator (`registerAccelerator: false`); if it is clicked with the mouse, it quits directly (no confirmation for an explicit choice in the menu).
- **Without a window in front** (no focused `webContents`, for example all windows closed): since no `before-input-event` arrives, keep a fallback global ⌘Q shortcut active only in this case, which applies the two-press rule.
- **Quitting by other means** (Dock menu, logout, shutdown, internal `app.quit()`): never a confirmation.
- **Setting**: Settings › General, "Confirm before quitting (hold ⌘Q or press twice)" switch. Off: ⌘Q quits immediately as before.

## v15: tab groups

Tabs opened from a Today tab are put with it in a group: a card with a header, like a folder. Contract: `Item.group?` (single tab or split view), `Space.groups?: Record<id, TabGroup>` (`name`, `hue`, `open`), `DragSource` `{ kind: 'group' }`, `DropTarget.groupId?`, `renameGroup`, `toggleGroup`, `showGroupMenu`. Code: packages/shared/src/tree.ts (utilities), `app/desktop/src/main/browser/group-actions.ts` and `model/groups.ts`, app/desktop/src/renderer/src/tabs/groups.ts, `GroupRow` (TabRow.tsx).

- **Model**: the Today list stays flat; a group is a run of at least two consecutive items carrying the same `group`. `syncGroups` (on each `changed()` and on load) keeps the invariant: a member detached from its run or a group reduced to one item loses the label; each live group has a header (created if needed), no header without a group. Never a label outside the Today tabs.
- **Creation**: link opened in a new tab (`target=_blank`, `window.open`, middle click, ⌘click, "Open in New Tab" in the context menu) from a Today tab of the current space (`openTab(url, { openerId })`): the new tab goes to the end of the opening tab's group, created around it if needed. From a pinned tab or a favorite: an ordinary Today tab, at the top. Default title: that of the opening tab (chosen name, page title, otherwise domain); hue: the first of blue, green, purple, orange, pink, turquoise, yellow, red that no other group in the space uses.
- **Header** (sidebar): tinted folder card; icon of the first tab, title in the hue, arrow; no cross (closing the group goes through its menu). Click: collapse / expand (when collapsed, the group's active tab stays visible); double click: rename in place; right click: Rename, Color, Collapse / Expand, Ungroup, Close Group; dragging the header: moves the whole group (Today tabs of any space, never in the middle of another group; not into pinned tabs or favorites).
- **Tabs of the group**: tabs like any other (closing the first one only closes it). Menu: "Remove from Group" (it goes right after the group). Drag and drop: dropped in the card (between two tabs, below the last one, or on the lower half of the header), the item joins the group; elsewhere, it leaves it.
- **Close the group**: all its tabs join the ⇧⌘T stack with the same `batch` (and the header on the first one); ⇧⌘T reopens them together, grouped, with their title and color. A tab closed on its own comes back into its group if the group still exists.
- **Pin / unpin**: "Pin Group" (group menu) turns it into a folder at the end of the pinned tabs: same name, emoji, hue and collapsed state; its tabs become pinned (`pinnedUrl`). "Unpin" (menu of a folder that contains tabs) does the reverse: its tabs, with subfolders flattened, become a group at the top of the Today tabs, with the folder's name, emoji, hue (a free hue if the folder has none) and collapsed state. Drag and drop: a group dropped into the pinned tabs (root or in a folder) becomes a folder at that spot; a folder dropped into the Today tabs becomes a group at that spot (never in the middle of another group).
- **Icon**: "Choose an Icon…" in the group menu (as for a tab and a folder) opens the emoji picker (sidebar panel, or popover below the label when tabs are on top); the emoji (`TabGroup.icon`) replaces the first tab's icon in the header and shows in front of the label's title. Same thing for a folder (`Item.folder.icon`, instead of the folder icon, everywhere it appears). Shared contract: `IconTarget`, `setIcon(target, emoji | null)`, `onPickIcon` (channel `icon:request`), popover `{ kind: 'icon', target }`.
- **Archiving**: automatic, a group leaves as a whole when none of its tabs has been used within the delay; "Archive" and its undo keep groups and headers.
- **Tabs on top**: the group is a tinted background that brings together a label (title in color; click: collapse / expand, drag: the whole group, right click: group menu; renaming goes through the field below the label) and its chips; when collapsed, only the active chip remains.
- Check: `SIGNET_GROUPCHECK=<folder>` (isolated profile required).

## v16: bookmarks, shortcuts on every system

### Bookmarks (per space)

Contract: `Space.bookmarks?: Bookmark[]` (`{ kind: 'link', id, title, url, favicon? }` or `{ kind: 'folder', id, name, items }`, nested folders), `BookmarkTarget` (`folderId?`, `index`), `Settings.showBookmarksBar?` (missing = shown), popover `{ kind: 'bookmarks', folderId, from?, tabId }`, renaming `{ kind: 'rename', target: { kind: 'bookmark', id } }`, API `toggleBookmark`, `addBookmark`, `moveBookmark`, `openBookmark`, `renameBookmark`, `showBookmarkMenu`, `showBookmarksBarMenu`, `onRenameBookmark`. Code: app/desktop/src/main/features/bookmarks.ts, app/desktop/src/renderer/src/toolbar/BookmarksBar.tsx.

- **Bar**: 32 px below the address bar of each pane (same colors, the site's), absent in compact mode; the web card moves down by as much (`BOOKMARK_BAR` in the layout). Bookmarks of the active space, each favicon + title (180 px at most); a folder: icon, name, small arrow. What does not fit goes into the "»" menu on the right. Empty: a sentence explains how to add some.
- **Open**: click on a link = in the pane's tab; ⌘click (Ctrl+click on Windows and Linux) or middle click = new background tab. Click on a folder or on "»": menu above the pages (overlay), subfolders as submenus opened on hover. The overlay popovers now work in both tab layouts.
- **Add**: ⌘D (adds the page at the end of the bar, or removes its bookmark if there is one, wherever it is stored); star to the right of the address (filled, in the accent color, when the page is bookmarked); tab dragged from the sidebar or the top row and dropped on the bar (before / after a bookmark, into a folder through its middle); tab menu "Add to Bookmarks" / "Remove from Bookmarks". While a tab is being dragged, the split view zones start below the bookmarks bar.
- **Organize**: drag a bookmark to move it, or drop it in the middle of a folder; never a folder into itself.
- **Menus**: right click on a link (Open, Open in New Tab, Rename…, Delete Bookmark, New Folder), on a folder (Open All in New Tabs, Rename…, Delete Folder, New Folder), on the bar (New Folder, Hide Bookmarks Bar). Rename opens a small field below the bookmark (or below "»" if it is hidden).
- **Show / hide**: ⇧⌘B, View menu, Settings › General "Bookmarks bar".
- **Import**: favorites from Chrome, Brave, Edge and Safari go into the bookmarks of the active space: straight into the bar if it is empty, otherwise into a folder named after the browser at the end of the bar; the bar is shown. Arc still creates spaces.
- Check: `SIGNET_BOOKMARKCHECK=<folder>` (with `SIGNET_SEED=1`, isolated profile).

### Shortcuts on Mac, Windows and Linux

- Menus: `CmdOrCtrl` everywhere (⌘ on Mac, Ctrl elsewhere). Quit: ⌘Q on Mac, Ctrl+Q elsewhere (confirmation by held or double press included). Shortcuts that use Control on Mac and would conflict elsewhere: go to space 1-9 = ⌃1…9 on Mac, Alt+1…9 elsewhere; split view = ⌃⇧= on Mac, Alt+Shift+= elsewhere (Ctrl+Shift+= is zoom there). The hidden alias ⌘⇧S only exists on Mac.
- Texts: written the Mac way in the catalogs; an i18next post-processor (`platformKeys`, packages/shared/src/i18n.ts) writes them for Windows and Linux (⇧⌘T → Ctrl+Shift+T, ⌥⌘B → Ctrl+Alt+B, ⇥ → Tab; two identical Mac spellings elsewhere become a single one). The shortcuts table in Settings, the command bar and the hints (`shortcut()`, app/desktop/src/renderer/src/lib/util.ts) follow the same rule.
- ⌘D adds a bookmark; pinning the tab moves to ⌥⌘P.

### View menu: check marks

The toggles of the View menu are checkboxes (check mark on the left when the option is on): Show Sidebar (disabled with tabs on top), Compact Mode, Tabs on Top, Show Bookmarks Bar. The menu is rebuilt as soon as one of these states changes, whatever the path (`Browser.apply`).

## v17: passwords

Everyone chooses their manager in Settings › Passwords (`settings.passwordManager`, `signet://settings/passwords`): **Signet** (default), an extension from the store (Bitwarden, 1Password, Proton Pass, Dashlane, LastPass; `PASSWORD_EXTENSIONS` list) or **None**. With an extension, the row says whether it is installed and offers "Install" (`Extensions.installPasswordManager`); Signet's keychain no longer fills anything and no longer offers to save. No iCloud Keychain nor KeePassXC: their extensions go through native messaging, which Electron lacks.

### Signet's keychain (`passwords.ts`)

- `passwords.json` file (userData, `AtomicWriter`): `{ id, origin, username, password, createdAt, updatedAt, lastUsedAt? }`, password encrypted by `safeStorage` (key in the system keychain). Exact origin (`https://host[:port]`): an account is only used on that origin. If encryption is unavailable, nothing is saved (warning in Settings).
- No password in `Snapshot` nor `SettingsInfo`: the Settings section goes through `passwords:list` (without passwords), `passwords:reveal` / `passwords:copy` (Touch ID if available, then one minute of grace; the clipboard is cleared after 90 s if it still holds the password), `passwords:save`, `passwords:delete`, `passwords:import`, `passwords:export`.
- CSV import (Chrome, Safari, Firefox, Bitwarden, 1Password, LastPass, Proton Pass, Dashlane: columns recognized by their name); logins already present (same origin and same username) are kept. CSV export in Chrome's format, after Touch ID, file in 0600.

### In pages (`app/desktop/src/preload/passwords.ts`, frame script on all sessions, private ones included)

- Touching a sign-in field (password, or username of a form that has one, or `autocomplete=username`) shows the site's accounts below the field (closed shadow root); a real click or Enter fills username and password (native setter + `input` / `change` events). Never any filling on load.
- Form submission, Enter in a field, click on a button near a filled password: the script sends `pw:submit`. The origin is that of the sending frame as seen by main (`senderFrame.origin`), never a value from the page, and only tabs are served. A username sent alone (two-step sign-in) is kept for 5 minutes for the password page.
- The list only reacts to real gestures (`isTrusted`): a page that gives focus to the field itself cannot pick an account with simulated keys or clicks.
- Same password already saved: `lastUsedAt` only. Otherwise, if `offerSavePasswords` and the origin is not in `neverSavePasswords`, an offer: overlay in `password` mode (card at the top right of the page: Save or Update / Never for this site / Not now, click outside = later); the password stays in main. Little Signet, Peek or overlay already open: native dialog. Never an offer in a private window.

### Key button in the address bar

- Just before the bookmarks star, with Signet's keychain only (never in a private window): `TabInfo.passwordKey`, computed by main and never written to disk. `pending` (accent): a login entered on the tab's site is not saved yet; `saved` (neutral): the keychain has accounts for its origin.
- "Not now" or a click outside no longer drops the offer: it stays in memory in main (the password never leaves main) until "Save" or "Never for this site", a navigation to another site, the tab being closed, or 30 minutes. A single offer per tab (the last input wins).
- Click on the key (`passwords:key`): when pending, the offer opens again below the button (card with an arrow); otherwise, a "Saved passwords" card (overlay in `passwords` mode) with the site's usernames (never the passwords), "Copy" for each one and "Manage passwords…" (Settings › Passwords).
- The button follows the tab's navigation, accounts being added or removed (Settings, import, saving) and the change of manager.

## v18: page tools (print, save, page source, spelling, reader, translation)

### Print, save, page source (`pagetools.ts`)

- **Print…** (⌘P, File menu, right-click on the page, command bar `print`): the system print dialog (`webContents.print`), which also offers PDF on Mac.
- **Export as PDF…** (File menu, `save-pdf`): `printToPDF` (backgrounds printed, A4, or Letter for the regions that use it), save dialog in Downloads, name taken from the title.
- **Save Page As…** (File menu, right-click "Save As…", `save-page`): complete page (`.html` + resources folder) or single-file web archive (`.mhtml`), depending on the chosen extension.
- **View Page Source** (⌥⌘U, View menu, right-click, `view-source`): `view-source:<address>` in a new tab right after it (in its own window for a private tab). Web pages and files only.

### Spelling

- Right-click on an underlined word: up to 5 suggestions (`replaceMisspelling`), "No Suggestions" otherwise, then "Add to Dictionary" (`addWordToSpellCheckerDictionary`), before the edit commands.
- Settings › General › "Check spelling" (`settings.spellcheck`, missing = on): `setSpellCheckerEnabled` on all sessions. On Mac, the system spell checker picks the language; elsewhere, the spell checker languages follow the preferred site languages (3 max).

### Reader mode (`reader.ts`, isolated world 1003)

- On each load (and 900 ms after an in-page navigation), `isProbablyReaderable` (Mozilla's Readability, bundled as `?raw` text) tells whether the page looks like an article: `TabInfo.reader = 'available'`. The same check reads `<html lang>` for translation.
- Book button in the address bar (accent while the reader is shown), ⌥⌘R, View menu, right-click, command bar `reader`. Readability extracts the article from a copy of the document; the content is cleaned (no script, no `on*` handler, no `javascript:` address, no form, no frame) then laid over the page in a closed shadow root: the page's address and state don't change, page scrolling is suspended.
- Reader bar (dimmed until hover): text size (14 to 28 px), Serif / Sans serif, theme (automatic, light, sepia, dark), column width, Quit (or Esc). The choices are kept for all pages (`settings.reader`, `DEFAULT_READER`): the view sends them to main through successive `next()` calls.
- A navigation closes the reader; an in-page navigation does too, then the page is checked again.

### Translation (`translate.ts`, isolated world 1004)

- Offered when the language the page declares is not among the preferred languages (`TabInfo.foreignLang`): button in the address bar. Also in the View menu, the right-click menu ("Translate to English") and the command bar, for any web page. Target language: the first preferred site language (Simplified or Traditional Chinese kept).
- In place, without reloading: the texts are grouped by paragraph (closest ancestor that isn't an inline element), each node tagged `<a i=n>`, sent in batches (6000 characters) to Google's service (`translate_a/t`, HTML format) from main; the translated pieces come back into the same text nodes, in document order. Only the nodes' values change, never the structure (React and the like stay intact); if the language moves words around, the sentence reads correctly but a link can slide by one word. The tab title is translated too.
- Visible paragraphs first (`IntersectionObserver`, one screen height of margin), then as the user scrolls and the page adds content (`MutationObserver`). The button breathes during the first translation, accent afterwards; a second click restores the original page. Network failure: original page and a message.
- The page text goes to Google only on request (never automatically).

## v19: customizable shortcuts

### Command list (`app/desktop/src/shared/shortcuts.ts`)

- `SHORTCUTS`: one entry per customizable command (identifier, Settings group, label key, default shortcut in Electron format, Mac / other systems variant, extra default shortcuts such as ⇧⌘] for the next tab). The same list builds the menu (`menu.ts`, function `command(id, item)`), the Shortcuts section of Settings, and the shortcuts shown by the command bar (`shortcutLabel`) and the toolbar tooltips.
- Commands with no default shortcut, to be chosen: Translate page, Save page as…, Export as PDF.
- Not customizable, shown read-only: ⌘1…9, ⌃1…9, Settings (⌘,), Full screen, Quit (⌘Q, see v14). Reserved (refused for a command): those and the Edit menu (undo, redo, cut, copy, paste, select all), Hide, Hide Others, Minimize to Dock.

### Settings (`settings.shortcuts`)

- Only the differences are saved: chosen accelerator, `null` for no shortcut; missing key = default shortcut (with its extras, lost as soon as another one is chosen). Main validates the whole set (`cleanOverrides`: known command, valid accelerator with ⌘, ⌃ or ⌥ except F1…F24, no duplicate, nothing reserved) and refuses everything if it is inconsistent; the menu is rebuilt when the differences change (`Browser.apply`).
- Shortcuts section: click on a command's chip → "Type the shortcut…"; the next key becomes the shortcut (letter read from the keyboard layout, AZERTY included, through `navigator.keyboard.getLayoutMap`). Esc cancels, Backspace removes the shortcut ("None"). Shortcut already taken by a command: "Already used by …" with Cancel / Replace (the other command loses its shortcut); reserved: "Reserved for …". A circular arrow restores the default shortcut of a changed row; "Reset all" clears all the differences.
- While recording (`shortcuts:recording`), the menu is rebuilt without registering any shortcut with the system, and ⌘Q is no longer intercepted (`pauseQuitShortcut`): ⌘W or ⌘Q reach the recorder instead of closing or quitting. Recording stops (and everything comes back) on Esc, a click elsewhere, loss of focus or closing the window.

## v20: screenshot, gradient spaces

### Screenshot (`capture.ts`, overlay `capture`, `Capture.tsx`)

- Opening: ⇧⌘2 (File menu, customizable), right-click on the page "Take Screenshot…", command bar `capture`. In the main window, the overlay dims the window and sits on the tab's web view (`rect`); elsewhere (Little Signet, private window), the visible part is offered for saving right away.
- Dragging picks an area (1:1 tracking, dimensions shown under the area, light edge; the dimming is the area's drop shadow). Bar at the bottom of the page, like the macOS screenshot tool: Area / Visible part / Full page, then Copy (Enter), Save…, Cancel (Esc).
- Area and visible part: `capturePage` (screen sharpness). Full page: debugging protocol (`Page.getLayoutMetrics` then `Page.captureScreenshot` with `captureBeyondViewport`), capped at 16,384 px per side.
- Copy: main renders the image as PNG (data URL), the overlay puts it on the clipboard (`navigator.clipboard.write`, main's clipboard API no longer accepts images), then shows "Copied to clipboard" and closes. Save: the overlay closes, save dialog in Downloads ("Screenshot <site> <date> at <time>.png").
- Permissions: the free permissions (clipboard write, full screen, pointer lock) are granted before any origin check, as the permission check already did; the interface, loaded from a file, has no web origin.

### Gradient spaces

- `Space.theme` (missing = solid color, as before): `{ hues: 2 or 3 hues, grain: 0 to 1 }`. `Space.hue` stays the first hue (accent, selections). Drafts coming from the interface are validated (`cleanSpaceDraft`, store.ts).
- Space form: Solid / Gradient segmented control with a preview next to it. Solid: the earlier swatches. Gradient: a click adds a hue (numbered in gradient order; beyond 3, the last one is replaced), a second click removes it; at least 2 hues to save; Grain slider.
- Main window: `linear-gradient(155deg, …)` background of the three hues mixed into the base background, amount multiplied by `--gk` (2.6 for a gradient, 1 for a solid color) and by the Appearance hue setting. Grain: noise texture (SVG `feTurbulence`) under the content, in `overlay`. `--h2`, `--h3`, `--gk`, `--grain` are registered and animated properties like `--h`: a solid space sets the three hues equal, so moving from a solid space to a gradient one slides smoothly (`useSpaceTheme`).
- Library: the space's swatch shows its gradient.

## v21: live folders

A pinned folder that fills itself from a source and updates on its own (`livefolders.ts`, feed reading in `feeds.ts`).

- Sources (`LiveSource`): an RSS 2.0 or Atom feed (blog, YouTube channel, releases of a GitHub repo…), or a regular page that advertises one (`<link rel="alternate" type="application/rss+xml|atom+xml">`); GitHub pull requests "Reviews requested from me" or "My open pull requests" (search API, with a personal token).
- Model: folder `Item` + `live: { source, refreshedAt?, error?, refreshing?, seen? }` (read back at startup without `refreshing`). Its children are regular pinned tabs, one per address (`pinnedUrl`): an existing tab keeps its state from one update to the next. The 15 most recent items (by date if the source gives one, otherwise in its order). An item gone from the source is removed, unless its tab is shown (active or pane of a split view).
- "New": opened addresses are kept in `seen`; the other tabs carry a dot (`TabInfo.liveUnread`, computed by main, not written). On creation, everything is already "seen".
- Update: 8 s after startup, every 15 minutes, and "Refresh" (folder menu). `net.fetch` with a 15 s timeout, 5 MB max. A failure keeps the previous items and records the error: small badge on the folder icon, message in its tooltip; an arrow spins on the icon during the update.
- Creation: right-click on the sidebar's "New folder" button, space menu, or command bar "New live folder". The form (overlay popover, in both layouts) offers "Feed or site" (address) or "GitHub" (choice of query); the source is checked before creation, the folder takes the feed's title. "Edit Source…" reopens the same form.
- A live folder is managed: no dropping tabs into it, no subfolder, absent from "Add to Folder". Icon of its source (`Rss`, `GitPullRequest`) unless an emoji is chosen.
- GitHub token: Settings › General › Live folders. Checked with GitHub (`/user`), then saved encrypted by `safeStorage` in `github-token.bin` (userData, 0600); never in state.json nor sent to a view (the interface only knows whether it exists). Link to the token creation page with `repo` access.

## v22: notes and Easels

Documents that live in the sidebar like tabs (pinned, Today, dragged, archived, in split view): `signet://note/<id>` and `signet://easel/<id>` (UUID).

### Opening and addresses

- Creation: File menu "New Note" (⌃⌘N, customizable, Ctrl+Alt+N elsewhere) and "New Easel" (no default shortcut), command bar `new-note` / `new-easel`, right-click on the sidebar's "New tab" (menu New tab / New note / New Easel). The document opens in a new tab of the active space.
- The tab keeps the `signet://…` address; it loads the app interface on the `#note/<id>` or `#easel/<id>` route (`resolveInternal` → `noteAppUrl`, and `displayUrl` does the reverse). Title and icon come from the page (`document.title`, SVG favicon as a data URL): the sidebar, history and command bar pick them up with no code of their own.
- Closing or archiving the tab deletes nothing: reopening the address (history, archives, command bar) finds the document again. Only "Delete" (in the page, with confirmation) erases the file and closes its tabs.

### Storage and security (`notes.ts`, `app/desktop/src/preload/notes.ts`)

- One file per document in `userData/notes/<id>.json` (`AtomicWriter`, written when the app closes). Note: `{ title, html }`, 2 MB max. Easel: `{ title, items, view }`, 30 MB max (images included, shrunk to 1600 px per side as WebP).
- Page script `signet-notes` (space sessions, not private windows): it only acts in the main frame of an interface page opened on a document route, and exposes a reduced `window.signet` (language, appearance) and `window.signetNotes` (`load`, `save`, `remove`).
- Main serves the `notes:*` channels only to the main frame of a tab whose real address is the app interface on a document route, and takes the id from that address (never from an argument): a page only touches its own document. A web page can't open `signet://note/…` (internal address guard) and has neither `window.signet` nor `window.signetNotes`. Easel items are validated again in main (types, bounds, colors, images as data URLs).
- HTML of notes and Easel texts cleaned on display and on paste (`notesHtml.ts`: text tags, http(s) and mailto links, http(s) or data images); the interface's security policy also forbids any inline script.

### Note (`Note.tsx`)

- `contentEditable` editor in a column, `<p>` paragraphs; the first line is the title (heading style, tab title). Bar dimmed until hover: Heading, Subheading, Bold, Italic, Strikethrough, bulleted, numbered and check lists, Quote, Code block, Link (⌘K, address typed in the bar), Delete.
- Shortcuts at the start of a line: `# `, `## `, `- `, `1. `, `[] `, `> `, ` ``` `. Checklist: click the box. ⌘-click on a link: new tab.
- Saved 400 ms after the last keystroke, and when leaving the page.

### Easel (`Easel.tsx`)

- Infinite board with a dot grid: tools Select (V), Text (T), Pencil (P), Image; six colors (text and strokes); zoom − / % / + (the % goes back to 100 %).
- Two-finger scroll: pan; pinch or ⌘ + scroll: zoom around the pointer; drag the background (Select tool): pan. Double-click on the background: new text; double-click on a text: edit it (empty on leaving = deleted).
- Dragging an item moves it; handle: size (proportions kept for images and strokes, width only for texts); Backspace, or the "Delete item" button (Delete key icon, visible only with an item selected, so it isn't confused with the trash at the top that deletes the Easel): delete. Text tool: a click creates the text and puts the cursor in it (the click has no default action, otherwise focus went back to the background and the empty text disappeared). Pasting or dropping an image (or text) adds it; "Add image" opens a file picker.
- Screenshot tool: "Easel" button next to Copy and Save: the screenshot opens a new Easel (image shrunk to 1600 px per side).
- Editable title at the top left; saved 600 ms after each change (view included).

## v23: tab preview, mods

### Tab preview on hover (`preview.ts`, view `#preview`, `previewHover.ts`)

- Resting 600 ms on a tab row of the sidebar (fixed or floating) or on a chip of the top row shows a card: thumbnail of the page if known, title, domain, and the "Playing" (sound playing) and "Asleep" (unloaded tab) badges. A card already shown moves from one tab to another with no delay; leaving the tabs removes it after 140 ms. No preview during a press, a drag, a rename, a scroll, nor when the window loses focus.
- Placement: next to the sidebar, on the page side (to the left for a sidebar on the right), aligned with the row; under the chip for the top row; always inside the window. The card lives in a small transparent view that main places above the pages and floating bars, under the overlay; it is never under the pointer (it disappears as soon as the pointer leaves the tabs). Enter: 6 px slide from the bar + fade (`--t-fast`), exit along the same path; reduced motion: fade only. The view is loaded ahead of time, at the first layout.
- Thumbnails: `capturePage` shrunk to 560 px wide (JPEG, in memory only), taken 900 ms after a tab appears, on hover of a shown tab, and every 15 s for shown tabs when the window has focus. 60 max (the oldest go), forgotten when the tab closes; never for a private window tab.
- Settings › Appearance › "Tab preview on hover" (`settings.tabPreview`, missing = on).

### Interface mods (`mods.ts`, Settings › Mods)

- A mod = a name and a CSS sheet, each one can be turned on separately. The active mods, in list order, are combined into one sheet inserted (`insertCSS`) into every view that shows the app interface (detected on each `dom-ready` by its address, `isAppUrl`): main window, floating bars, overlay, Settings, Library, Little Signet, private windows, picture in picture, find, preview… Never in web pages. A change replaces the sheet everywhere right away (the interface itself is the preview).
- File `mods.json` (userData, `AtomicWriter`): `{ disabled, mods: [{ id, name, css, enabled }] }`, 200,000 characters max per mod.
- Settings section: "Turn off all mods", list (move up, move down, export as `.css`, edit, delete with in-place confirmation, switch), "Import…" (name taken from a `/* name: … */` header or from the file name), "New mod"; monospace editor (⌘S saves, Tab inserts two spaces, Esc cancels); help that lists the hook points (`--h`, `--accent`, `--bg`, `--surface`, `html.route-main`, `.sidebar`, `.toolbar`, `.tabstrip`, `.bm-bar`, `.palette`).
- Fallback if a mod makes the interface unusable: View menu › "Interface Mods" (checkbox, native: no CSS can hide it), and launching with `--safe-mode` (or `SIGNET_SAFE_MODE=1`), which applies no mod for the session (the section says so).

## v24: backup and restore

Settings › Backup (`signet://settings/backup`, backup.ts, BackupSettings.tsx). A `.signetbackup` file: compressed JSON (gzip) `{ format: 'signetbackup', version: 1, createdAt, appVersion, sections }`, written as 0600.

- Parts, each one with a checkbox (`BACKUP_SECTIONS`): settings, favorites, spaces and pinned tabs (folders and live folders included), Today tabs, bookmarks, history, archives, Boosts, site permissions, Mods, notes and Easels, passwords, extensions (their identifier, reinstalled from the store on the launch after the import). Never: cookies and sessions, cache, download list, GitHub token. Tabs are saved by their content (address, title, icon, chosen name and emoji), not by their identifier; Today tabs and bookmarks keep a reference to their space (id, name).
- Passwords: unchecked by default; checking them asks for a backup password (6 characters min, confirmed) and Touch ID. They are decrypted from the keychain then encrypted again in the file (scrypt N = 2¹⁵ + AES-256-GCM); the rest of the file is in plain text.
- Import: choose a file → summary (date, version, parts present with their count) → check the parts → "Add to what's there" or "Replace what's there" → backup password if passwords are checked → confirmation → restart.
  - Add: nothing is erased, nothing is added twice (same tab address, same bookmark, same origin and identifier, same Mod, same note id; history merged by address). A space with the same id receives the pinned tabs it lacks.
  - Replace: each checked part replaces its own. Settings are always replaced, whatever the choice.
  - Today tabs and bookmarks go into the space with the same id, otherwise the same name, otherwise the active space. Tabs that nothing references any more are removed from the state.
- Applying: the app writes everything waiting in memory, makes a full safety copy without passwords (`userData/backups/safety-<date>.signetbackup`, the last 3 kept), changes the profile files, then `app.relaunch()` and `app.exit(0)` (without the on-close saves, which would write the old state back). The next load validates everything again (state, bookmarks, keychain, notes).
- Safety copies listed in the section: "Restore…" opens them like a chosen file (Replace mode preselected); "Show folder".

## v25: sites installed as apps

Like Chrome ("Install page as app"): `webapps.ts`, route `#webapp` (`WebApp.tsx`), overlay `webapp` (`WebAppInstall.tsx`), Settings › Apps (`WebAppSettings.tsx`).

### Installing

- Entry points: File › "Install This Site as App…" (customizable shortcut `installApp`, none by default), tab menu, right-click on the page (outside links, images and fields), command bar. Only an http(s) page of a main window tab (otherwise a dialog explains it). If an installed app already covers the address, the entry becomes "Open in “name” App" and the page moves there.
- Inspection (isolated world 1005): manifest (`link rel=manifest`, read with the tab's session, 512 KB max), declared icons, `application-name`, `theme-color`. Start address = the manifest's `start_url` if it is on the same origin, otherwise the page without fragment; scope = the manifest's `scope` (same origin, containing the start), otherwise the origin. Name = `name`, `short_name`, `application-name`, page title, host.
- Icon: maskable ones of at least 192 px first, then the others (manifest, `apple-touch-icon`, page icons, tab favicon, `/favicon.ico`); above 512 px the smallest, below the largest; 4 tries max, 5 MB max. It reaches the overlay as a data URL.
- Overlay (card at the top right of the page, like the password prompt): icon preview, editable name, Cancel / Install. The icon is composed in a 1024 px canvas following the macOS grid (824 px plate, 185 px corners, shadow): filled by the icon if it is maskable or square, opaque and sharp (≥ 120 px), otherwise placed at 60 % on a white background with an edge; with no icon, the name's initial on the space's hue.
- Custom icon: click on the icon (＋ badge on hover) or "Choose Image…", drop an image on the card, or paste it (⌘V). PNG, JPEG, WebP, GIF, SVG, ICO or BMP, 15 MB max (otherwise "Unreadable image"). It replaces the site icon; three framings: Fill (the whole rounded plate), Fit (at 60 % on the white plate), As Is (the image alone over the whole square, for an icon already drawn for the Mac). Suggested framing: Fill for an opaque square of at least 120 px, otherwise Fit. "Site Icon" (or "No Image") goes back to the icon found.
- Install: the app is saved (`userData/webapps.json`, icon `userData/webapps/<id>.png`), the page moves into the app window (a Today tab closes, a pinned one stays), then the launcher is created.

### App window

- `BrowserWindow` without tabs: 38 px bar (route `#webapp`, traffic lights at `{14, 12}`) then the page over all the rest, no margin. Size and position kept per app; several windows of the same app cascade by 24 px. Session: the one of the space where the app was installed if it is isolated, otherwise the shared profile.
- Bar in the site's colors (the page's `themeColor`, otherwise the manifest's): Back, Reload / Stop, page icon and title in the center, Copy URL, "Open in Signet" (the page becomes a Today tab, the window closes). Page outside the scope (sign-in, external link): domain badge (padlock on https, red otherwise) followed by the title, and a "Back to <app>" button that reloads the start address.
- Links: `target=_blank` and `window.open` inside the scope → new app window; outside the scope → tab in the main window; popups with `features` (OAuth) → real windows. Context menu without Peek or split view; "Open in New Tab" goes to the main window.
- Menus: the app window behaves like a Little Signet (⌘W closes it, ⌘R, ⌘[, ⌘], zoom, ⇧⌘C, developer tools target its page; tab commands bring back the main window). The window title is the page's.
- Closing the window forgets the tab. App windows are not reopened on the next launch.

### Launcher

- Bundle `~/Applications/Signet Apps/<name>.app` (`SIGNET_WEBAPPS_DIR` replaces it for checks): `Info.plist` (identifier `dev.signet.app.webapp.<id>`, `LSUIElement`), `app.icns` (`iconutil`, 16 to 1024 px), `launcher` script. Written in a hidden folder then renamed; LaunchServices is notified (`lsregister -f`). A name already taken gets "2", "3"…; a bundle whose identifier isn't the app's is never deleted or replaced.
- Packaged version: the script runs `open -b dev.signet.app signet://webapp/<id>` (`signet` scheme declared in `build.protocols`); development: it relaunches Electron with `--signet-webapp=<id>`, which hands over to the open instance (`second-instance`).
- Launch received (`open-url`, `second-instance`, argv at startup): app window brought to the front, or a new window on the start address. Cold start from a launcher: only the app window opens (the Dock brings back the main window). Unknown app: main window and a "This app is no longer installed" dialog.
- Limitation: a launcher doesn't stay open; in the Dock and ⌘⇥, the app window shows under the Signet icon.

### Settings › Apps

List (icon, name, domain, profile space, "launcher deleted"): Open, Rename (in place; the launcher is recreated under the new name), Show in Finder (recreates the launcher if it is gone), Uninstall (in-place confirmation; closes its windows, deletes the launcher and the icon). The command bar offers "Open “name” app" for each app. Channels: `webapp:install`, `webapps:list|open|rename|reveal|remove`, `webapp:ready|home|open-in-main|copy-url`; events `webapp:state`, `webapps:changed`.

## v26: built-in developer tools

Contract: `TabInfo.devtools`, `DevToolsDock`, `DevToolsPrefs` (`Settings.devtools`), `Pane.devtools`; channels `devtools:dock`, `devtools:close`, `devtools:size`, `devtools:menu`. Module devtools.ts, card DevToolsCard.tsx.

- Each tab has its own tools, in a `WebContentsView` created by Signet: `setDevToolsWebContents` then `openDevTools({ mode: 'detach' })`. A fresh view on each opening (a tools view already used can't be reused); closing the tools closes its `webContents`.
- Three placements: **right** of or **below** the page (separate card inside the pane card, separated from the page by the split view's 8 px gap), or **in a window** (title "Developer Tools - host", position kept; title bar hidden: the window loads the `#devtools` route, a 36 px header with the traffic lights, the same title and the same buttons as the built-in card (Right and Bottom attach the tools to the page, grayed out if the tab isn't in the main window), and the tools view below). The same view moves from one placement to another without reopening the tools.
- Layout: the pane card is split before the pane bar is computed; the page keeps 320 px of width or 200 px of height, the tools 320 px or 160 px (half and half if there isn't enough room). The built-in tools have a 36 px header (icon, "Developer Tools", Right / Bottom / Window, close; on the right the header is a `drag` area), view with radius 12 below. Split view: each pane can have its tools. Picture in picture mode: nothing for the page that is in it.
- Divider between the page and the tools: drag to resize (size in px, applied live, kept at the end of the drag), double-click for the default size (480 px on the right, 320 px at the bottom).
- Placement and sizes: those of the last choice (`Settings.devtools`, right by default), for each new opening; the open/closed state and the placement stay specific to each tab, in memory only (nothing is reopened on restart).
- Opening: ⌥⌘I (toggle), palette ("Developer Tools", "… on the right", "… at the Bottom", "… in a Window"), context menu "Inspect" (opens the tools then selects the element in them). The tools take focus when they open and when they change placement; on close, focus goes back to the page.
- Pane bar: `</>` button (accent) as long as the tab's tools are open, whatever their placement; click → native menu (Dock to Right, Dock to Bottom, Separate Window, Close Developer Tools).
- Little Signet, app window, private window, Peek: tools always in a window. A tab moved to another window with its built-in tools: ⌥⌘I shows them in a window instead of closing them.
- Tools window: the menu commands target the inspected page (⌘R reloads the page, ⌥⌘I closes the tools, ⌘W closes the window and the tools).
- A link opened from the tools (`devtools-open-url`) opens a tab. A tab whose tools are open is never unloaded automatically.
- Development screenshots: `SIGNET_DEVTOOLS=right|bottom|window` opens the active tab's tools; their view is composed into the screenshot.

## v27: developer mode

Contract: `Settings.devMode`, `Settings.devKeepOutOfHistory`, `TabInfo.consoleErrors`, `TabInfo.consoleWarnings`; channel `devtools:console`; shortcut `jsConsole` (⌥⌘J). Modules devmode.ts and app/desktop/src/shared/devhosts.ts; section Settings › Developer (`signet://settings/developer`).

- Local pages (`isLocalDevUrl`): http(s) on `localhost`, `*.localhost`, `*.local`, `*.test`, `127.x.x.x`, `::1`, `0.0.0.0`, and local network addresses (`10.x`, `192.168.x`, `172.16` to `172.31`).
- Settings › Developer: "Developer mode" switch (missing = off; it also shows the Develop menu, see v28); when it is on, "Keep local pages out of history" (missing = saved); reminder of the shortcuts for the tools (⌥⌘I) and the console (⌥⌘J), as they are set.
- Pane bar, mode on and local page: the port follows the host, in secondary color (`localhost:5173 / title`). Console counter: solid red badge (number of errors) and amber badge (warnings), only if one of the two isn't zero, 99+ beyond; tooltip "3 errors · 1 warning · Open the console"; click → tools opened on the console. Also shown in the narrow bar.
- Counting: `console-message` messages of error or warning level (uncaught exceptions included), plus failed resources (`webRequest.onCompleted` with a status ≥ 400, `onErrorOccurred` except `ERR_ABORTED` and `ERR_BLOCKED_BY_CLIENT`), like the Chromium console. Reset on each new page (main frame navigation that changes document); in-app navigations of a single-page app keep the count. Broadcast batched, every 150 ms at most. Turning the mode off clears the counters; nothing is counted before it is on.
- History: with the option, a local page isn't saved (so it isn't offered by the command bar either); open tabs stay searchable.
- JavaScript console (⌥⌘J, View menu, palette, counter): opens the tab's tools in their usual placement (or shows them if they are open) and shows the Console panel. The tools restore their last panel while loading: the requested panel is shown after `devtools-opened`.

## v28: Develop menu

Contract: `DevOverrides`, `DevNetwork`, `DevUserAgent`, `TabInfo.devOverrides`; channel `develop:menu`. Module develop.ts.

- App **Develop** menu (between Navigation and Window, as in Safari), only with developer mode: Developer Tools, JavaScript Console; Empty Cache and Reload (cache of the tab's whole session, then reload without cache); Clear Site Data and Reload (cookies and storage of the page's origin); then the tab settings: Disable Cache, Disable JavaScript, Page Theme (Follow System / Force Light / Force Dark), Reduce Motion, Network (No Throttling / Fast 4G / Slow 4G / 3G / Offline), User Agent (Default / Safari Mac, iPhone, iPad / Chrome Android / Firefox Mac); Reset Tab Settings. The commands target the tab of the frontmost window; the checkmarks follow the active tab of the main window (the menu is rebuilt when it changes or when its settings change).
- Tab settings: specific to the tab, as long as its view exists (closing or unloading the tab clears them), never saved. Applied through the debugging protocol already attached for languages (languages.ts): `Network.setCacheDisabled`, `Emulation.setScriptExecutionDisabled`, `Emulation.setEmulatedMedia` (`prefers-color-scheme`, `prefers-reduced-motion`), `Network.emulateNetworkConditionsByRule` (empty pattern = all requests; `Network.emulateNetworkConditions` as a fallback), `Emulation.setUserAgentOverride` (always with the languages, applied again after a language change; matching `navigator.platform`). The Network domain is only enabled while a cache or network setting is on. Disabling JavaScript or changing the user agent reloads the page.
- Networks: Chromium preset values, i.e. Fast 4G 165 ms, 9 Mb/s ↓, 1.5 Mb/s ↑; Slow 4G 562.5 ms, 1.6 Mb/s ↓, 750 kb/s ↑ (throughput × 0.9); 3G 2 s, 500 kb/s (× 0.8); offline. User agents: Safari 26 (Mac, iPhone, iPad in mobile version), Chrome Android and Firefox Mac at the current Chromium version (+ 3 for Firefox).
- Pane bar: flask button (accent) as long as at least one setting is on for the tab; tooltip "Developer settings active: JavaScript disabled, 3G"; click → native menu of the tab settings (without Empty Cache or Clear Data).
- Command bar, developer mode: the same commands for the active tab, worded according to the state ("Disable JavaScript in This Tab" / "Enable…", "Simulate Network: 3G", "User Agent: Safari (iPhone)"…), flask icon.
- Development screenshots: `SIGNET_DEVELOP=<JSON DevOverrides>` applies settings to the active tab and shows what the page sees of them (`window.__o`); `SIGNET_NETTIME=1` times the load of a fresh address.

## v29: responsive design mode

Contract: `DevDevice`, `DevResponsive`, `DevOverrides.responsive`, `Pane.responsive`, `Settings.responsive`; channel `develop:responsive`; shortcut `responsive` (⇧⌘M, Develop menu). Device list: app/desktop/src/shared/devices.ts; interface: Responsive.tsx.

- Devices (CSS px in portrait, density, mobile): iPhone SE 375×667 ×2, iPhone 16 393×852 ×3, iPhone 16 Pro Max 440×956 ×3, Pixel 9 412×923 ×2.625, Galaxy S24 360×780 ×3, iPad mini 744×1133 ×2, iPad Pro 11" 834×1210 ×2 (all mobile, with the Safari iPhone/iPad or Chrome Android user agent); Laptop 1280×800 ×2, Desktop 1920×1080 ×1; Custom size (not mobile, 200 to 3840 px).
- Enter/exit: ⇧⌘M, "Responsive design mode" checkbox in the Develop menu and the flask menu, palette. On entering: the last device used (`Settings.responsive`, iPhone 16 at first). Specific to the tab, like the other developer settings (the flask recalls it: "responsive mode (iPhone 16)").
- Layout (main, `Develop.place`): in the pane's page area (under the bar and bookmarks), a 40 px bar at the top, then the web view at the device size, shrunk if there isn't enough room (scale ≤ 1, 16 px margin on the sides and at the bottom), centered horizontally, rounded corners. Around it, the area takes `--surface-2`. `Pane.rect` is the web view; find in page and screenshots are placed in it.
- Emulation (debugging protocol): `Emulation.setDeviceMetricsOverride` (size, density, mobile, display scale, screen, portrait/landscape orientation for a mobile device), `Emulation.setTouchEmulationEnabled` and `setEmitTouchEventsForMouse` for a mobile device. Sent again only when the device, size or scale changes; cleared on exit. The page is reloaded when the user agent or the mobile flag changes (touch and user agent are only read on load). Without a `viewport` tag, a mobile page is laid out on 980 px, as on a real phone.
- Bar (renderer): device list (Phones, Tablets, Computers, Custom size), editable width × height (Enter or leaving the field; Esc cancels; switches to Custom size), Rotate (swaps width and height), scale shown if < 100 %, Quit.
- Handles in the margin: on the right (width, on both sides since the page is centered), at the bottom (height), in the corner (both); dragging switches to Custom size, sent on each frame.
- Development screenshots: the screenshot of a view in responsive mode covers the emulated size, the page shrunk to the top left; the composition crops it to the view size.

## v30: JSON viewer

Contract: `Settings.jsonViewer` (missing = on). Modules jsonview.ts (main) and app/desktop/src/main/devtools/pages/jsonview.js (page script bundled as text, `?raw`, linted with the browser globals).

- When: at the end of a tab load (`did-finish-load`, http(s) and file pages), if `document.contentType` is `application/json`, `application/…+json` or `text/json`, the document is the `<pre>` that Chromium shows for a JSON response, the text is 20 MB at most and it parses with `JSON.parse`. Otherwise the page stays as is (invalid JSON included).
- Where: isolated world 1006 (the page sees neither the script nor the interface); interface in a closed shadow root laid over the page; styled with a constructed sheet (`adoptedStyleSheets`), which works even with a strict CSP (`default-src 'none'`, GitHub API). Light or dark colors following the page theme.
- Bar: Tree / Raw (the text received) / Pretty (indented by 2 spaces); in tree view, Expand all, Collapse all, search field and number of results; on the right, Copy JSON (pretty).
- Tree: keys, strings, numbers, booleans and null in colors; http(s) strings as links; collapsed containers summarized ("{…} 3 keys", "[…] 12 items"); empty container shown as `[]` or `{}` with no arrow. Click on the arrow, double-click or ⌥-click on the row: collapse/expand. Document of 3,000 nodes at most: fully expanded; beyond that, the root and the first level. Large containers are shown in slices of 500 children ("Show more (n left)").
- Selection: click on a row → bottom bar with the path (`$.items[3].label`, `$["special key"]`), Copy path, Copy value (indented JSON for a container, plain text for a string).
- Search (150 ms after typing): keys and values that contain the text, case-insensitive, 1,000 results max; matching rows highlighted, parents expanded and slices extended up to the result; Enter / ⇧Enter: next / previous (current row marked more strongly, centered). Going back to tree view returns to the current result.
- Copy: "copy" command on a temporary text area (no clipboard access request), "Copied" for 1.2 s.
- Settings › Developer: "JSON viewer" switch (independent of developer mode).
- Development screenshots: `SIGNET_JSONTEST=<script>` runs in the viewer's world (`window.__signetJson`: its shadow root) and shows the result.

## v31: unpacked extensions

Contract: `Settings.unpackedExtensions` (folders), `ExtensionInfo.unpacked`; channels `extensions:load-unpacked`, `extensions:reload-unpacked`, `extensions:remove-unpacked`. Module unpacked.ts.

- Settings › Extensions, with developer mode (as in Chrome, so that a user can't be tricked into loading just anything): "Unpacked extension" row and "Load Folder…" button (folder picker). Without the mode: a note points to Settings › Developer. Extensions already loaded stay loaded and listed even if the mode is off.
- Loading: the folder must contain `manifest.json` (otherwise the alert "The folder … has no manifest.json"); `loadExtension(folder, { allowFileAccess: true })` in the default session, carried over to the other sessions like a store extension (and to a session opened later). A failure (invalid manifest…) opens an alert with Chromium's message. The folder is kept and reloaded on each launch, after the store extensions.
- Automatic reload: the folder is watched (recursive `fs.watch`); 400 ms after the last change, the extension is removed then loaded again (same identifier, derived from the path). `.git`, `node_modules`, `.DS_Store` and editor temporary files are ignored. After a failure, the next change tries again. As in Chrome, the content scripts of pages already open only change when the page reloads.
- List: "Unpacked" (and "reloaded at HH:MM:SS" after a reload), the folder on one line truncated from the start (full path in a tooltip, `~` for the home folder), the error in red if loading failed (the extension is then listed without an identifier); Reload and Remove buttons ("Remove it? Its folder stays on disk.").
- Removal: the extension is unloaded from all sessions and forgotten; its folder is never erased (the store removal, which erases the installed folder, isn't used). Same thing from "Remove" in an extension button's menu.
- Development screenshots: `SIGNET_UNPACKED=<folder>` loads the extension, changes `content.js` (v1 → v2), checks the reload on the active page, then removes it (`SIGNET_UNPACKED_KEEP=1`: keeps it).

## v32: local names

Contract: `DevAlias`, `Settings.devAliases`. Shared functions in app/desktop/src/shared/devhosts.ts: `isAliasName`, `aliasTarget`, `resolveAlias`, `aliasFor`.

- Settings › Developer, "Local names" (independent of developer mode): `name → address` list with Remove, and an add row (name, address, Add; Enter confirms). Errors in place: invalid name, name already taken, invalid address.
- Name: lowercase letters, digits and hyphens (63 characters max, no hyphen at the start or the end), optionally followed by `.test` or `.localhost` (names reserved for testing); never a real domain (`.com`…), so that no site is hidden and nothing passes for it.
- Address: only a local address (`isLocalDevUrl`); `3000` → `http://localhost:3000`, `localhost:5173` and `127.0.0.1:3000/app` completed with `http://`; no query or fragment, no trailing `/`. Main validates again (`settings.update`): invalid entries dropped, only one per name (the first).
- Input (command bar, address field): `name`, `name/path`, `name?query` or `http(s)://name/…` opens the target followed by the rest, before any other interpretation (`mysite.test` would otherwise be taken for an https domain). The command bar also offers the names that start with the input ("Local name → address").
- Pane bar: a page under a name's target (same origin, equal or deeper path) shows the name instead of the host and port ("mysite / title"); the real address in a tooltip. History and the address field keep the real address.

## v33: engine settings

Electron does not ship Chromium's settings page (`chrome://settings`, `chrome://flags`, `chrome://extensions` all fail with `ERR_INVALID_URL`), so the engine settings that Electron can drive live in Signet's Settings window (`features/engine.ts`):

- **Settings › System › Use graphics acceleration when available** (`settings.hardwareAcceleration`, missing = on): read from `state.json` before `ready` and applied with `app.disableHardwareAcceleration()`, so it takes effect at the next launch. While the saved choice differs from the running engine (`SettingsInfo.hardwareAccelerationActive`), the packaged app offers "Relaunch"; in development the row only says it applies at the next launch (relaunching would detach the app from electron-vite).
- **Settings › System › Proxy**: Signet follows the system proxy; the button opens the macOS Network settings, Windows `ms-settings:network-proxy`, or on Linux `gnome-control-center network` or the KDE proxy module (row hidden when none is installed).
- **Settings › Privacy › Secure DNS** (`settings.secureDns`, `settings.secureDnsServer`): with the current provider (`automatic`, the default), off, or a chosen DNS over HTTPS server (Cloudflare, Google, Quad9, NextDNS, CleanBrowsing, or a custom `https://` template, `@shared/dns`). Applied live with `app.configureHostResolver` for the whole app; left to the engine's default until the user chooses.

## Linux and Windows

Signet is designed for macOS but must work on Linux and Windows (app/desktop/src/main/app/platform.ts).

- Windows: `windowChrome()`: on Mac, hidden title bar, positioned traffic lights, translucent background (`vibrancy`); elsewhere, the system frame and buttons, hidden menu bar (Alt shows it), opaque background in the theme. Mac-only APIs (`setWindowButtonVisibility`, `setWindowButtonPosition`, `app.dock`, Touch ID, `defaults`) are guarded by `IS_MAC`.
- Translucency (Settings › Appearance): no effect outside Mac, backgrounds stay opaque (`appearance.ts`).
- Sites installed as apps: Mac only (the launcher is a macOS app); the "Install as App" entries are hidden elsewhere.
- Closing: outside Mac (no Dock to reopen the app), closing the last visible or minimized window quits the app (index.ts); on Mac, the app stays open as usual. Without this, the app kept running with no window and relaunching it, even in a newer version, only woke up the old one.
- Wayland (Linux): the app runs under XWayland (`--ozone-platform=x11`), because Wayland forbids placing a window and keeping it on top: picture in picture (position, drag, resize, always on top) wouldn't work. The option must be there when the process starts (Electron picks the platform before our code): when packaging for Linux, `scripts/after-pack.mjs` renames the executable to `signet-bin` and puts in its place a script that adds the option when `DISPLAY` exists under a Wayland session. An `--ozone-platform=…` passed by hand takes precedence. No relaunch from the app: Chromium starts the new process without elevation rights (`no_new_privs`) and the AppImage FUSE mount fails ("Cannot mount AppImage").
- AppImage (app/desktop/src/main/app/linux.ts): on each launch, shortcut `~/.local/share/applications/signet.desktop` (up-to-date AppImage path, web types for the default browser, line `X-Signet-AppImage=true`; a shortcut without it is never replaced) and icon `~/.local/share/icons/hicolor/512x512/apps/signet.png` (chosen variant), referenced by its full path in `Icon=` (some docks don't read the icon theme again after they start). Windows also get the chosen icon.
- Import (Settings › Import, command bar): only the system's browsers: Arc on Mac and Windows, Safari on Mac, Chrome, Chromium, Brave and Edge everywhere (system package, Flatpak or Snap on Linux). Data selection window (spaces, favorites, passwords, extensions, history; all checked), password decryption specific to each system (Keychain, GNOME keyring, DPAPI): see app/desktop/src/main/features/importers/FORMATS.md. The note about full disk access only appears on Mac.
- App menu: outside Mac, the menu bar is hidden (Alt shows it); a ⋮ button at the end of the toolbar opens the whole app menu under the pointer (`showAppMenu`).
- Blank interface views (guard.ts `healUiView`): an interface view whose preload fails (`preload-error`) or whose page doesn't load (`did-fail-load`) is reloaded, 1 s, 2 s, 4 s… 30 s max; the page itself reloads if `window.signet` is missing (main.tsx). Typical case: `out/` rewritten by a build during launch. One log line per failure (not per try). In development, a connection refusal from the interface server (`ERR_CONNECTION_REFUSED` on `ELECTRON_RENDERER_URL`) means the dev session is over (Ctrl+C): the app quits instead of trying again.
- Dropdowns (`<select>`): outside Mac, the list takes the element's background, translucent in our case; the `option`s have an opaque background (`--win-bg`).
- Packages: `npm run dist` (Mac, .dmg), `dist:linux` (AppImage x64), `dist:win` (NSIS installer x64). On an Apple Silicon Mac without Rosetta, the AppImage is built in Docker (`electronuserland/builder:wine`); the Windows installer, though, needs Wine, which crashes under emulation: it is built by GitHub Actions (`.github/workflows/build.yml`, run by hand or on a `v…` tag, packages for all three systems in the artifacts).

## Out of scope

Sync, several main windows, find in page in Little Signets, any AI feature.

## Animated surface exits

Palette, site panel, popovers (overlay), sidebar panels (downloads, space and icon forms) materialize (scale 0.96 → 1 + opacity, from their anchor) and leave along the same path in `--t-fast` (120 ms; plain fade with reduced motion). All closing paths go through `closeOverlay`: main keeps the overlay view in place for 120 ms (`OVERLAY_EXIT_MS`) then removes it; a new opening cancels the exit. Peek and drop zones close without animation. Running a palette action doesn't wait for the exit (it starts immediately; only the view fades out).

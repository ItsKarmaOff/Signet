# Status after automated checks

Run on macOS (debug bundle) with `scripts/selftest.sh`: a local HTTP site, a throwaway data folder (`SIGNET_DATA_DIR`), no keychain
(`SIGNET_NO_KEYCHAIN=1`), no phone link (`SIGNET_LINK_OFF=1`), `SIGNET_SELFTEST=1`. All checks pass (see `scripts/selftest.sh`, 55 at the last run), quit is clean (exit 0,
no `Signet Helper` left). `cargo test --workspace` passes.

Passed automatically: DevTools (docked right and bottom, window mode, console panel, inspect element, only the frontend origin loads in the tools view, close and reopen: `ONLY=devtools`; `SIGNET_TEST_SHOTS=<folder>` saves screenshots of the tools view), startup and first-run import, tab load and view rectangle, page script bridge and `swipe:can` round trip, closing a tab keeps the
window, command bar overlay (and its network suggestions event), find, sidebar toggle and slide, split view (two panes, no overlap), private window,
Little Signet, note (`signet://note/<id>` loads and answers `notes:load`), JSON viewer, ad blocker (needs the filter lists, downloaded at first use),
HTTPS-only upgrade, Peek (open, expand), Settings on a section (and again on another), Library, Passkey, Boost and Welcome windows (open and close), install
as app prompt, responsive mode (393x852 screen emulation), capture to an Easel, mini player (open, close), unpacked MV3 extension (background page,
content script `sendMessage` round trip, popup round trip, popup close).

Not automated (manual): passwords and anything that needs the real keychain, the Electron vault import, phone link and passkeys over the phone,
  web app install (writes outside the data folder), downloads, printing, permissions prompts, Windows and Linux.

DevTools do not use `ShowDevTools` (it aborts the process): they load the bundled frontend from Chromium's remote debugging server on a random
127.0.0.1 port. While the app runs, any local process can drive the browser through that port (documented in `crates/signet-web/README.md`);
`SIGNET_NO_DEVTOOLS=1` turns it off. Not checked: Windows and Linux, the Cmd+Opt+I shortcuts inside the tools view (app shortcuts are swallowed by the key filter).

Changes the extensions rely on: pages of extensions are navigated under the alias scheme `signet-extension://` (Chromium blocks navigations to unknown
`chrome-extension://` ids); signet-web maps URLs back, so the app only sees `chrome-extension://`. The extension bridge uses `__signetCrxDispatch`.

Launch (release): `cd app/desktop-rs && ./scripts/bundle-macos.sh --release && open dist/Signet.app` (or run `dist/Signet.app/Contents/MacOS/signet` in a terminal
to read the logs; add `SIGNET_DEBUG=1`). Reset the Rust data folder for a fresh import from the Electron folder: quit the app, then
`rm -rf "$HOME/Library/Application Support/Signet GPUI"` (the Electron `Signet/` folder is only read). Automated run: `scripts/selftest.sh [seconds]`
(`ONLY=ext,devtools` limits it to groups; windows open on the screen).

Startup timeline: `SIGNET_TRACE_STARTUP=1 dist/Signet.app/Contents/MacOS/signet` prints `[startup] <ms since the process was created> (+<ms since the
previous line>) <phase>` on stderr: data folder, store, GPUI start, CEF load and initialize, child process launches (the helpers print
`[startup:<type>]` lines on the same time base), UI init, menus, main window, first frame, first browser and its request context, first
`load_start` / `load_end`, and main thread stalls over 50 ms during the first 15 seconds. Without the variable the marks cost one atomic load.
A first launch of a freshly built or copied bundle spends 2 to 3 s before `main.entry` (macOS checks the new ad hoc signed bundle).

# Manual test checklist: Signet in Rust

For the person who tests the app once everything is assembled. Derived from [docs/MANUAL-CHECKS.md](../../docs/MANUAL-CHECKS.md) and
[docs/SPEC.md](../../docs/SPEC.md). Each line is "what to do -> what should happen". Shortcuts are given for macOS; on Linux and Windows
replace Cmd by Ctrl (and Option by Alt). Keep the Electron app (`app/desktop`) at hand to compare behavior.

Before you start:

- [ ] Build and bundle: `scripts/bundle-macos.sh --release` (or the Linux / Windows script). The bundle opens: `dist/Signet.app`.
- [ ] Have a terminal ready to read the app's output, and note the exact steps of every failure.
- [ ] Quit the Electron app first (both can run, but the first-run import is easier to judge alone).

Legend: **Mac** = macOS only, **Lin** = Linux, **Win** = Windows.

## 1. Startup and first-run import

- [ ] Delete `~/Library/Application Support/Signet GPUI/` (keep `Signet/`), launch the app -> one main window, no crash, no blank interface, no leftover helper process after quitting.
- [ ] Spaces, favorites, pinned tabs, Today tabs, folders, groups and bookmarks of the Electron app are there, in the same order and colors.
- [ ] History: type a word you visited before in the command bar -> history rows show up. Settings and shortcuts customized in Electron are kept.
- [ ] Notes and Easels of the Electron app open with their content.
- [ ] The Electron folder `Signet/` was not modified (compare modification dates).
- [ ] Quit and relaunch -> the same state, window size and position come back; the import does not run again.
- [ ] Brand new user (rename both folders away): onboarding shows, you can choose a search engine, and the app ends on an empty window.
- [ ] Launch a second instance (`open -n` or run the exe again) -> the first one gets focus (single instance); with an http(s) URL as argument it opens in a Little Signet.
- [ ] Start in safe mode (`SIGNET_SAFE_MODE=1`) -> the app starts without extensions and scripts, and says so.
- [ ] Idle CPU with 5 tabs open and nothing playing stays low (see section 30).

## 2. Window, sidebar, tabs

- [ ] Resize, move, maximize and full screen (green button, F11) the window: the sidebar, toolbar and page area follow with no flicker, pages are not cropped or offset.
- [ ] Mac: traffic lights are well placed; Lin/Win: the system frame and buttons show, the menu bar is hidden and Alt shows it, and a "..." button at the end of the toolbar opens the whole app menu.
- [ ] Cmd+S hides and shows the sidebar; hovering the left edge while hidden shows the floating sidebar, moving away hides it; the floating toolbar does the same at the top.
- [ ] Drag the sidebar edge -> it resizes within its limits and the width is remembered after a relaunch.
- [ ] Cmd+T opens the command bar, a new tab appears at the top of Today tabs and is active; Cmd+W closes it and activates a sensible neighbor.
- [ ] Cmd+Shift+T reopens the last closed tab (several times in a row, in the right order).
- [ ] Cmd+1 to Cmd+8 go to the tab at that position (sidebar order, favorites first), Cmd+9 goes to the last tab; Ctrl+Tab and Ctrl+Shift+Tab (and Cmd+Shift+] / [) cycle through favorites, pinned tabs, then Today tabs.
- [ ] Each tab shows its favicon, title, loading spinner, audio icon when a page plays sound, and a close button on hover.
- [ ] Rename a tab (context menu) -> the new name shows; clear it -> the page title comes back.
- [ ] Pin a tab (Option+Cmd+P) -> it moves to the pinned area; the pinned tab keeps its address when you navigate away (back-to-pin behavior as in Electron); unpin works.
- [ ] Add a tab to favorites (command bar action or context menu) -> it shows in the favorites grid; remove it.
- [ ] Drag and drop: reorder tabs, drag a tab onto a folder, onto pinned and favorites areas, to another space in the sidebar bottom bar; drop indicators show and the final order is right.
- [ ] Tabs left in Today are archived after the delay chosen in Settings > General ("Archive Today tabs after"); "Archive Today's Tabs" in the command bar does it at once; archived tabs show in the Library and can be restored.
- [ ] Unused background tabs are unloaded to save memory and reload when you come back (the tab shows as dimmed or asleep, content returns).
- [ ] Undo: close a tab or delete a folder, then use Undo (toast with "Undo", or the menu) -> it comes back.
- [ ] Middle click on a link opens a tab in the background; Cmd+click opens a tab; Shift+click or a `target=_blank` link opens a new tab; `window.open` popups (OAuth sign-in) open a real popup window and close properly.
- [ ] Two-finger swipe left and right on the trackpad (Mac) goes back and forward, with the same feel as Electron (v10).
- [ ] Mouse back and forward buttons navigate.
- [ ] Context menu of a tab: every entry present in the Electron app (rename, pin, favorite, mute, duplicate, move to space, close others, copy URL...) and each one works.

## 3. Spaces

- [ ] Create a space (command bar "New Space", or the + in the bottom bar): name, color, icon; it appears and becomes active.
- [ ] Gradient space: the gradient and the grain are visible to the eye (zoom in if needed); change the colors, they update live.
- [ ] Switch spaces by clicking, with the shortcuts (Cmd+Option+Left / Right if configured, or the "Go to Space" actions) and with the trackpad swipe on the sidebar bottom bar; each space keeps its own tabs, favorites, pinned tabs, folders and bookmarks.
- [ ] Isolated space (own cookies and storage): sign in to a site in one space, open it in another: you are not signed in. A normal space shares the default session.
- [ ] Edit and delete a space (with its confirmation); deleting moves nothing unexpected; the last space cannot be deleted.
- [ ] Reorder spaces by drag and drop.
- [ ] Space with its own default search engine or settings (if present in Electron): honored.

## 4. Folders, groups, pinned folders

- [ ] Create a folder, rename it, color it, expand and collapse it (state remembered after relaunch).
- [ ] Put tabs in a folder by drag and drop; remove them; nested folders behave like in Electron.
- [ ] Pinned folders (v3): a pinned folder keeps its tabs across archive cycles and is shown in the pinned area.
- [ ] Tab groups (v15): create a group from selected tabs, name and color it, collapse and expand it, add and remove tabs, ungroup, close the group.
- [ ] Live folders (feed folders) are covered in section 20.

## 5. Split view

- [ ] Ctrl+Shift+= (or "Split View" in the command bar) opens the command bar in split mode: choose a result -> two panes side by side; with a 3rd pane, a 4th request does nothing (max 3).
- [ ] Choosing an existing open tab moves it into the split view. On a favorite or with no tab, the result opens as a normal tab.
- [ ] Drag a tab on the page area -> drop zones show (left, right, top, bottom); dropping creates the split.
- [ ] Drag the divider between panes -> sizes change live; double click resets (if Electron does); the pane bar shows each page's title, with a close button and swap buttons.
- [ ] Vertical and horizontal layouts, and a 3 pane layout: all rendered like Electron, rounded card corners, no web page overflowing its pane.
- [ ] Focus: clicking a pane makes it the active one (highlighted pane bar); keyboard shortcuts act on the active pane.
- [ ] "Leave Split View" from the command bar or the pane bar -> panes become separate tabs.
- [ ] Resize the window and toggle the sidebar while split: panes follow.

## 6. Command bar

- [ ] Cmd+T: empty field shows the 6 most recent open tabs of the space and a few actions. Typing filters; the top row is what Enter runs.
- [ ] Type `exam` -> pages from tabs, history and archive rank like in Electron (start of host first); `youtube` puts youtube.com first; free text shows "Search for ... on <engine>" and up to 4 DuckDuckGo-style suggestions after it.
- [ ] Type `example.com`, `localhost:3000`, `192.168.1.1`, `signet://settings` -> open as addresses, not searches.
- [ ] Cmd+L: field prefilled with the current address, selected; Enter navigates the current tab; Esc closes.
- [ ] Every action works: New Space, Split View, Leave Split View, Show/Hide Sidebar, Pin, Favorite, Copy URL, New Little Signet, Archive Today's Tabs, Close Tab, Reopen Closed Tab, Reload, Developer Tools, toggle Peek links, Set as Default Browser, Go to Space <name>, Settings, private window, and the later actions of v5 to v33.
- [ ] Local aliases (v32, local names): a typed alias opens its address, prefix suggestions show.
- [ ] The bar opens and closes with the fade and scale animation; clicking outside closes it; opening it during a Peek closes the Peek first.
- [ ] Arrow keys move the selection, Tab / Shift+Tab as in Electron, Cmd+Enter or Alt+Enter variants if the Electron bar had them.

## 7. Peek

- [ ] Click an external link in a pinned tab (setting on) -> Peek opens: dimmed background, framed page with rounded corners, Close and "Open as tab" buttons.
- [ ] Esc, a click on the dimmed area and the Close button close it and discard the tab; "Open as tab" moves it to the top of Today tabs and activates it.
- [ ] Peek from the context menu of a link, and from the command bar if available. Only one Peek at a time. Title updates while the page loads.
- [ ] Keyboard focus is inside the Peek page (typing in a field works).

## 8. Little Signet

- [ ] Option+Cmd+N opens a small window (760x820) cascaded 24 px from the previous one; bar with back, forward, title and domain, "Copy URL", "Open in Signet".
- [ ] "Open in Signet" moves the tab to the main window (top of Today, active) and closes the small window; closing the small window discards the tab.
- [ ] A link opened from another app (`open https://example.com` on Mac when Signet is the default browser, or `signet https://example.com`) opens in a Little Signet.
- [ ] Menu shortcuts (Cmd+W, Cmd+R, Cmd+[, Cmd+], Cmd+L) act on the Little Signet when it is in front. No find in page there.

## 9. Private windows

- [ ] Shift+Cmd+N opens a private window; several windows are cascaded; the "Private" badge shows; no spaces, favorites or folders.
- [ ] Sign in to a site in a private window: not signed in in a normal window; two private windows share nothing; closing the window forgets cookies.
- [ ] Nothing is added to history; Shift+Cmd+T does nothing; no extensions run.
- [ ] Both layouts (sidebar and tabs on top) follow the main window's setting; Ctrl+Shift+S switches it.
- [ ] Address field: Enter navigates, Esc cancels; Cmd+T, Cmd+W, Cmd+L, tab cycling and Cmd+1..9 work; Cmd+F does nothing; closing the last tab closes the window.
- [ ] A download made in a private window shows while the window is open and is gone when it closes.
- [ ] A popup login (OAuth) stays in the private session.

## 10. Find in page

- [ ] Cmd+F opens the find bar over the page; typing highlights matches and shows "n of m"; Enter / Shift+Enter move between matches; Esc closes and clears the highlights.
- [ ] Case and whole-word options (if Electron has them) work; the search text is kept when reopening in the same tab; in a split view it targets the active pane.
- [ ] Find in a PDF and in a long page works and scrolls to the match.

## 11. Downloads

- [ ] Click a download link -> a download starts, the sidebar downloads panel/button shows progress, speed and size; the file lands in the Downloads folder (no overwrite: `name (1).ext`).
- [ ] Pause, resume, cancel; open the file; show it in Finder / Files / Explorer; remove from the list.
- [ ] Right click an image or link > "Save Image As" / "Download Link" works.
- [ ] A finished download survives a relaunch in the list; "Clear data > Downloads list" empties it.
- [ ] A page that triggers many downloads, a blob: URL and a data: URL download all work.

## 12. Ad blocker

- [ ] Settings > Privacy > "Block ads and trackers" on: visit a news site with ads -> ads are gone; the toolbar or site panel shows a blocked count.
- [ ] Off: ads come back after reload. The setting survives a relaunch.
- [ ] YouTube video ads and cosmetic filtering (hidden banners) behave like in Electron; check that no site breaks (login pages, payment pages).
- [ ] Per-site allow (site panel) works if present.

## 13. Bookmarks and history

- [ ] Cmd+D bookmarks the page in the current space (toast or star state changes); again removes it (or edits it, like Electron).
- [ ] Library > Bookmarks: list, search, folders, rename, delete, open, drag to reorder; bookmarks are per space.
- [ ] Library > History: entries grouped by day, search, open, delete one entry, clear all; a site visited in a private window is absent.
- [ ] Library > Archive (archived tabs): restore and delete.
- [ ] The library opens as its own window or panel exactly like Electron and remembers its tab.
- [ ] Import bookmarks from Chrome/Arc/Safari (Settings > Import, section 15) shows up here.

## 14. Site panel, toolbar, address bar

- [ ] The toolbar shows back, forward, reload (turns into stop while loading), the address (domain at rest, full URL on focus), the key button, extension buttons, and the "..." / app menu on Lin/Win.
- [ ] Back and forward are disabled when there is no history; long-press (or right click) shows the history list if Electron does.
- [ ] Click the lock/site icon -> the site panel shows connection info, permissions, cookies, zoom, ad blocker state and Boost for this site; changes apply.
- [ ] Toolbar blends into the site color (v7): visit a site with a strong top color, the toolbar takes it and text stays readable in light and dark.
- [ ] Certificate error page (`https://expired.badssl.com`, `https://self-signed.badssl.com`): an error page shows, with the option to continue if Electron offers it.
- [ ] Error pages: offline, unknown host (`http://nonexistent.invalid`), connection refused (`localhost:1`): readable page with a retry.
- [ ] Zoom: Cmd+Plus, Cmd+Minus, Cmd+0 act on the page; the zoom bubble shows the value; per-site zoom is remembered (see section 15 for default zoom).
- [ ] Full screen from a page (YouTube): the page fills the window, sidebar and toolbar are hidden, Esc leaves it and everything comes back with the right layout.
- [ ] Context menu on a page: link (open in new tab, Peek, private window, copy, save), image (open, copy, save, copy address), selection (copy, search, Look up on Mac), text field (cut/copy/paste, spelling suggestions), page (back, forward, reload, view source, inspect, save as, print).
- [ ] Status text: hovering a link shows its URL at the bottom of the window.
- [ ] Hover preview (v23): hover a background tab in the sidebar for about a second -> a preview card with a screenshot and title; not shown for the current tab nor for panes of a split view.

## 15. Settings

Open with Cmd+, , the gear in the sidebar bottom bar, or the command bar "Settings". The window is reused when already open.

- [ ] **General**: search engine, archive delay, Peek on pinned links, default browser status and "Make default" (works in the bundle: the system then opens links in Signet), live folder tokens, tab layout.
- [ ] **Appearance**: app icon variants (the grid of previews, the Dock icon changes at once on Mac, the Finder icon stays), theme (light, dark, system), translucency (Mac only; no effect elsewhere), sidebar side/width options, compact mode.
- [ ] **Privacy**: block ads and trackers, HTTPS-only, "Clear data" with History / Archived tabs / Downloads list / Cache / Cookies and site data and its inline confirmation; each box really clears its data.
- [ ] HTTPS-only on: `http://example.com` ends on `https://example.com`; `http://localhost:3000` and `http://192.168.x.x` stay on http; a site without https shows "This site doesn't support a secure connection" with "Go back" and "Continue to site" (allowed until quit, not in a private window); a DNS error still shows the normal error page.
- [ ] Fonts: Font size "Very large" and Minimum font size 16 -> the text of newly opened tabs is bigger; a visible tab keeps its size until reopened.
- [ ] Default zoom 150% -> a site you never zoomed opens at 150%; a site you zoomed by hand keeps its zoom; Actual Size gives 150%; `signet://` pages, settings and notes are not zoomed; hand zoom survives a relaunch; a zoom set in a private window is forgotten.
- [ ] **Languages** (site languages, v8) and **interface language** (English / French, v9): change them -> the interface changes at once; websites receive the right `Accept-Language`.
- [ ] **Import**: sources present on the machine are listed ("Not found on this Mac" in gray for others); the data selection window lets you pick spaces, favorites, passwords, extensions, history; Mac: the Full Disk Access note shows; the import adds the data and does not duplicate it on a second run.
- [ ] **Shortcuts**: every command listed by group; change one (for example Split view to Cmd+E) -> it works; while recording, Cmd+W and Cmd+Q do not act; a conflict is reported; "Reset all" restores the original shortcuts.
- [ ] **Boosts**, **Mods**, **Apps**, **Extensions**, **Devices**, **Passwords**, **Autofill**, **Backup**, **Developer**, **Engine**, **About**: each is covered in its own section below; each section opens, saves on change and is still set after a relaunch.
- [ ] **About**: version matches `Cargo.toml` (0.5.0), the repository link opens in a tab.

## 16. Passwords and autofill

- [ ] Sign in to a test site -> the "Save password?" offer shows; Save stores it in the keychain (Mac: Keychain item; the vault is unlocked with Touch ID or the system prompt where Electron did).
- [ ] Return to the site -> the login form is filled or offered; the key button in the address bar lists the logins; "Copy" puts the username on the clipboard; the password copy clears itself if Electron does.
- [ ] Offer shown, then click elsewhere: the key stays in the address bar and a click reopens the offer.
- [ ] Update a saved password (change it on the site) -> the update offer shows; "Never for this site" is remembered.
- [ ] Private windows also offer passwords (frame script on all sessions) but nothing leaks to the main window.
- [ ] Settings > Passwords: list, search, reveal (asks to authenticate), edit, delete, export (CSV) and import (from another browser) work; the passwords imported from the Electron app on first run are readable (or the app says clearly that they cannot be, and offers re-import).
- [ ] Autofill (addresses, cards): save an address in Settings > Autofill; on a checkout form the fields are suggested and fill correctly (name, address, phone, country, card fields); delete an entry.
- [ ] Autofill on pages in iframes and with unusual field names (autocomplete attributes) behaves like Electron.
- [ ] Generate a strong password on a sign-up form if Electron offers it.

## 17. Passkeys with a phone, phone link

Needs a phone with the Signet mobile app (or any iPhone / Android for passkeys) and Bluetooth on the computer.

Passkeys (Mac needs the bundle, since the terminal is not the responsible process for Bluetooth):

- [ ] On webauthn.io, "Register": the "Passkey" window opens with a QR code; Cancel, Esc and closing the window give the site an error and nothing hangs.
- [ ] Bluetooth off on the computer: the window says so; once on, the normal message returns (the scan never turns it on itself).
- [ ] Scan the QR with the phone and accept: "Phone found. Connecting...", then "Confirm on your phone.", then the window closes and webauthn.io confirms the registration.
- [ ] "Authenticate" on webauthn.io with that passkey -> signs in.
- [ ] Microsoft sign-in with a passkey from the phone works; without a phone, Cancel leads to "Use my password".
- [ ] Same flow from a private window: the QR window opens above the private window.
- [ ] Mac: the Bluetooth permission prompt shows once with the Info.plist text; after Allow, no prompt again. Lin: BlueZ detects the phone; if not, copy the `Passkey:` and `Bluetooth:` lines of the terminal. Win: WinRT Bluetooth works without a prompt.
- [ ] Conditional mediation (autofill UI passkeys) answers `NotAllowedError` at once, without a QR.

Phone link (Settings > Devices):

- [ ] Turn the link on, "Add a phone": a QR code shows with a timer; scan it in the Signet mobile app -> the phone appears as a device ("paired"); Mac: the Local Network prompt shows once (allow); Win: allow the Firewall prompt on private networks.
- [ ] Wrong code or expired QR: the phone gets an error and the QR is renewed on request; 5 wrong attempts cancel pairing.
- [ ] With the link on, local dev servers (start `python3 -m http.server 3000`) appear on the phone within about 5 s, and open there through the tunnel.
- [ ] "Send to phone" on a page (context menu / site panel) opens the URL on the phone; `localhost` URLs are mapped.
- [ ] A page opened from the phone arrives on the computer as a new tab (and the window comes forward).
- [ ] The four switches (general and device, on both sides): turning any one off stops the link; turning it back on restores it without re-pairing.
- [ ] Forget a device -> it disappears; the phone must pair again. Quit the app -> the phone sees the computer go away; the port is released.
- [ ] mDNS: the phone discovers the computer by name (`_signet._tcp`) on the same Wi-Fi.

## 18. Notes and Easels

- [ ] Create a note (command bar / sidebar): it opens in a tab at `signet://note/<id>`; the title and body save as you type and survive a relaunch.
- [ ] Rich text: headings, lists, checklists, bold/italic, code, links; Cmd+K on a selected word creates a link; paste keeps sane formatting; undo/redo.
- [ ] Create an Easel: draw with shapes, text, arrows, free draw; select, move, resize, delete; scroll with two fingers pans, pinch zooms (trackpad).
- [ ] Drag an image from Finder into an Easel and paste a copied image: it appears.
- [ ] Rename and delete notes and Easels; they show in the right place of the sidebar; export if Electron has it.
- [ ] Notes of the Electron folder show their content after the first-run copy. Notes pages cannot be reached by normal web pages (open `signet://note/<id>` from a site link: blocked).

## 19. Mods, Boosts

- [ ] Boost (Option+Cmd+B on a page): the editor opens; pick elements to hide, change colors/fonts/CSS; the page updates live; Save stores it for the site.
- [ ] Settings > Boosts: list with site, switch, number of hidden elements, date; Edit, delete (inline confirmation); empty state mentions Option+Cmd+B.
- [ ] A saved Boost applies on the next visit and in every tab of that site; the switch disables it; a Boost does not apply in private windows if Electron excludes them.
- [ ] Mods (v23, Settings > Mods): add a CSS mod for the interface, toggle it, remove it; the interface restyles live; a broken mod does not break the app (use "Reload Interface" in the View menu).
- [ ] Import a mod file / paste a mod if Electron supports it; export it.

## 20. Live folders

- [ ] Settings > General > Live folders: save a GitHub token (scope `repo`); invalid token shows an error.
- [ ] Create "GitHub > Reviews requested from me" and "My open pull requests" folders: they fill with items and icons; clicking an item opens it.
- [ ] RSS/Atom feed folder: add a feed URL; entries appear; unread state if Electron has it.
- [ ] Leave the app open for more than 15 minutes -> a feed folder updates on its own; a failing feed does not break the others.
- [ ] Delete a live folder and its token.

## 21. Web apps (sites installed as apps)

Mac only; the "Install as App" entries must be hidden on Lin/Win.

- [ ] Mac: on a site, "Install as App" -> a launcher app is created (default under `~/Applications`) with the site's icon and name; Settings > Apps lists it.
- [ ] Launch it from Finder / Spotlight: a window with only the site (app window, minimal chrome); it has its own Dock icon; links to other domains open in Signet.
- [ ] Quit the app window: the launcher exits; "Remove" in Settings > Apps deletes the launcher.
- [ ] Lin/Win: no install entry anywhere.

## 22. Extensions

- [ ] Install an extension from the Chrome Web Store (open a store page, Add): it installs; Settings > Extensions lists it; uBlock-style and password manager extensions run.
- [ ] Install an unpacked extension (v31) from a folder: it loads, reloads when files change if Electron does; errors are displayed.
- [ ] The pinned extension buttons show in the toolbar; click opens the popup in a proper window under the button; drag to reorder; the Extensions button lists all with a menu (pin, options, remove, inspect).
- [ ] Extension options pages, context menus, content scripts, `chrome.storage`, `chrome.tabs`, `chrome.alarms`, `declarativeNetRequest` (a blocking extension blocks), `chrome.commands` shortcuts work.
- [ ] NordVPN (proxy, privacy, alarms): installs, "extension APIs installed: proxy, privacy, alarms" shows in the log, no "Unknown Extension API" nor "Failed to start worker"; connect -> an IP-check site shows the VPN address in a default-space tab and in an isolated-space tab; a private window stays direct; disconnect restores the real address; connected state survives a relaunch; removing it while connected restores the address at once; a WebRTC leak test shows no leak; proxy authentication works; no offscreen window appears; the popup still opens after the extension opens and closes a tab; alarms keep running for several minutes.
- [ ] Extensions are absent in private windows. Removing an extension removes its button and data.
- [ ] Backup / restore with extensions reinstalls them on restart (section 28).

## 23. Developer tools, developer mode, Develop menu

- [ ] Option+Cmd+I opens the developer tools of the active tab (docked panel or window as in Electron's v26 setting); inspect element from the context menu jumps to the node; closing and reopening works; works in a split pane and in Little Signet.
- [ ] Settings > Developer / developer mode (v27): enabling shows the Develop menu (v28) with view source, JS console, user agent switching, disable cache, throttling, responsive mode, and so on; each item acts on the active tab.
- [ ] View page source opens a source tab with syntax highlighting; "Save as", "Print" (print preview and PDF) work.
- [ ] Terminal/log: console messages of pages are not flooding; the "interface.log" file exists in the data folder `logs/` and records interface crashes.
- [ ] Local names (v32): if configured, `myapp.test`-style names resolve to the local server and use http without the HTTPS-only warning.

## 24. Responsive design mode

- [ ] Develop > Responsive Design Mode (v29): the page shows inside a resizable frame with width/height fields; drag handles resize live; presets (phone, tablet, desktop) apply; rotate swaps dimensions.
- [ ] Device pixel ratio and user agent emulation apply (check `window.innerWidth`, `devicePixelRatio`, `navigator.userAgent` in the console); touch emulation if Electron has it; leaving the mode restores the page exactly.

## 25. JSON viewer

- [ ] Open a URL that returns JSON (`https://api.github.com`) -> the JSON viewer (v30) shows a collapsible tree, colored types, copy path/value, search/filter, raw/pretty toggle; large JSON stays responsive.
- [ ] A page with `application/json` but invalid JSON falls back to plain text. HTML pages are never taken over.

## 26. Menus, shortcuts, quit

- [ ] Application menu has Signet, File, Edit, View, Navigate, Develop (when enabled), Window with the accelerators of the SPEC; items are enabled/disabled correctly; accelerators work whichever pane or view has focus (page, sidebar, command bar).
- [ ] Edit menu: undo, redo, cut, copy, paste, paste and match style, select all work in the page, in the address field and in the command bar.
- [ ] View > Full Screen, Zoom, Reload, Force Reload (Shift+Cmd+R, bypasses cache), Reload Interface works.
- [ ] Key presses go to the page first only for keys the app does not own; shortcuts that web pages also use (Cmd+T inside a web editor) still run the app command like Electron.
- [ ] Press Cmd+Q once -> the "Hold Cmd+Q to quit" bubble shows; releasing keeps the app; holding Cmd+Q quits. Lin/Win: the quit confirmation as Electron does (close of last window quits).
- [ ] Mac: closing the window keeps the app in the Dock, clicking the Dock icon reopens it; Lin/Win: closing the last window quits.
- [ ] Quit leaves no `Signet Helper` / `signet-helper` process (Activity Monitor, `ps`); relaunch right after works with no "profile in use" error.
- [ ] Play a YouTube video then switch tab -> the floating mini player shows (v13), can be moved and resized, controls (play, pause, close, back to tab) work, and it closes when the video ends or the tab closes.
- [ ] Picture in picture from the page's own button also works.
- [ ] Permissions: a site asking for camera/microphone, location, notifications or clipboard shows the permission prompt with the right site name and wording; Allow / Deny persist per site (not in private windows); the system prompt for camera/microphone appears once on Mac with the Info.plist text.
- [ ] Media: camera and microphone work on a test page (`https://webcamtests.com`); screen sharing (Meet/Zoom web) works or fails gracefully; audio mute per tab works.
- [ ] Drag a file onto a page (upload areas) and a link/text between pages works; clipboard copy/paste with rich text.
- [ ] Printing: Cmd+P opens the print dialog; "Save as PDF" writes a correct PDF.
- [ ] Reader mode and translation (v18), if exposed: they work on an article / a foreign page.
- [ ] Spell check underlines mistakes and the context menu offers corrections; languages follow Settings.
- [ ] Screenshot (v20, capture overlay): select an area, save to file / copy to clipboard; whole page capture works.

## 27. Appearance and languages

- [ ] Light, dark and system themes: every surface (sidebar, toolbar, command bar, settings, library, notes, popovers, toasts, error pages) follows; switching the system theme live updates the app.
- [ ] All app icon variants (aurore, corail, graphite, menthe, nuit, ocean, or, papier) apply to the Dock/taskbar and the About page.
- [ ] Accent colors per space tint the sidebar and selection consistently; contrast is readable on every gradient.
- [ ] Language English then French: all labels, menus, tooltips, onboarding, error pages and dialogs change with no leftover keys (`some.key.name` visible anywhere = bug).
- [ ] Animations: surfaces open with scale 0.96 to 1 and fade, close in about 120 ms; with "Reduce motion" on in the system they become plain fades.
- [ ] Scale: on a Retina / 150% display text and icons are sharp; moving the window between two displays with different scales keeps pages sharp and aligned.
- [ ] If the sidebar and toolbar ever disappear: the interface comes back on its own, or View > Reload Interface does it; the `interface.log` says what crashed.

## 28. Backup and restore (Settings > Backup)

- [ ] Export with passwords: Touch ID / the system authentication is requested; a `.signetbackup` file is created and is not readable as plain text.
- [ ] Export without passwords works.
- [ ] Import in "Add to what's there" mode: the confirmation shows, Signet restarts, nothing is duplicated, old data is intact.
- [ ] Import in "Replace what's there" mode, then restore the safety copy from the same section -> the previous state returns.
- [ ] Import a backup that contains extensions: they reinstall on restart.
- [ ] A backup made by the Electron app opens in the Rust app (and the reverse, if supported).
- [ ] A corrupt or wrong-password file is rejected with a clear message and changes nothing.

## 29. Platform specifics

- [ ] Mac: Metal rendering, Retina, Touch ID prompts, Dock icon changes, default browser registration (System Settings > Desktop & Dock > Default web browser lists Signet), trackpad gestures, `open -a Signet https://example.com`.
- [ ] Linux: runs under X11 and under Wayland (through XWayland): windows, menus, mini player always on top and movable; a `.desktop` file launches it; default browser via `xdg-settings`; BlueZ passkeys; clipboard and drag and drop with other apps; HiDPI scaling (GDK_SCALE).
- [ ] Windows: DPI changes (move between monitors, change scaling) keep layout and sharpness; Alt shows the menu bar; title bar and window buttons; Firewall prompt on first phone link; `signet.exe https://example.com` hands over to the running instance; installer/ico shows correctly in the taskbar.

## 30. Performance and memory comparison with the Electron app

Use Activity Monitor (Mac; Task Manager details on Windows; `htop` / `ps` on Linux). Same machine, same data, one app at a time, both freshly launched, release build of the Rust app (`--release`), the Electron app packaged (not `npm run dev`).

Setup for both: the same account/data, 10 tabs open: google.com, a YouTube video (paused), a news site, GitHub, a web app (Gmail or Notion), a heavy page (maps), and 4 plain sites. Wait 2 minutes, then record values.

- [ ] Memory: in Activity Monitor, search "Signet" and add the memory of the main process plus every helper (`Signet Helper`, `(GPU)`, `(Renderer)`...). Note total memory (Memory column or "Real Mem") for Electron and for Rust. Expected: Rust is lower or equal; record the numbers.
- [ ] Number of processes: Rust app should have one renderer per site as Electron (same CEF/Chromium model); note any difference.
- [ ] Idle CPU with the window focused and nothing playing: should stay under about 1% (GPUI redraws only on change; CEF message pump idles at 0.3 to 0.5%).
- [ ] Idle CPU with the window in the background or hidden: close to 0%.
- [ ] CPU while scrolling a heavy page and while resizing the window: no sustained 100% on one core; no dropped frames visible.
- [ ] Energy impact column (Mac): not higher than Electron during idle and video playback.
- [ ] Startup time: from launch to usable window (stopwatch, cold and warm). Expected: faster than Electron.
- [ ] Open 50 tabs one after the other (command bar, same site list repeated): memory grows linearly, the app stays responsive, unloading of idle tabs reclaims memory.
- [ ] Switch tabs quickly with Ctrl+Tab held: no visible lag or blank flashes.
- [ ] Type in the command bar with a large history (10,000+ entries): suggestions appear in well under 100 ms.
- [ ] Long run: leave the app 1 hour with several tabs, check memory did not climb steadily (leak check) and no helper process is stuck at high CPU.
- [ ] Disk: size of the bundle (`du -sh dist/Signet.app`) vs the Electron app; size of the data folder after a month of use.
- [ ] Record the results here:

| Metric | Electron | Rust (release) |
| ------ | -------- | -------------- |
| Total memory, 10 tabs | | |
| Process count | | |
| Idle CPU, focused | | |
| Idle CPU, background | | |
| Cold start (s) | | |
| Warm start (s) | | |
| Bundle size | | |

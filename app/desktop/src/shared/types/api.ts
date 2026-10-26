import type {
  Appearance,
  ArchivedEntry,
  BookmarkTarget,
  DevResponsive,
  DevToolsDock,
  HistoryEntry,
  LibrarySection,
  LiveSource,
  Locale,
  PasswordManager,
  Rect,
  Settings,
  SpaceDraft,
  SplitLayout
} from '@signet/shared/types'
import type { BackupInfo, BackupMode, BackupSection, BackupSummary, ImportKind } from './data'
import type { DevToolsWindowState } from './devtools'
import type { LinkState } from './link'
import type { BoostEditorState, ModsInfo } from './mods'
import type { OverlayPayload, PaletteMode, PaletteResult, PopoverRequest, QuitHudState } from './overlay'
import type { PasswordImportResult, PasswordsInfo } from './passwords'
import type { BrowsingDataKind, SettingsInfo } from './settings'
import type { PermissionState, SitePermissionEntry } from './permissions'
import type { Snapshot, StatePatch } from './state'
import type { CaptureTarget, DragSource, DropTarget, FindState, IconTarget, TabPreviewPayload } from './tabs'
import type { LittleState, PipCommand, PipState, PrivateState, WebAppState, WebAppsInfo } from './windows'

/**
 * API exposed to the UI by the preload under `window.signet`.
 * Each method has an entry in the IPC contract (src/shared/ipc/contract.ts: channel, argument schema);
 * the preload builds `window.signet` from that contract and main registers its handlers in src/main/ipc/.
 */
export interface SignetApi {
  // State
  getState(): Promise<Snapshot>
  onState(cb: (s: Snapshot) => void): () => void
  onStatePatch(cb: (p: StatePatch) => void): () => void
  /** Hint from main: a space is being chosen elsewhere (overlay popover). */
  onPendingSpace(cb: (spaceId: string) => void): () => void
  /** ⌘L: main asks for the address field's rectangle; the answer goes through `openPalette('edit-url', rect)`. */
  onEditAddressRequest(cb: () => void): () => void

  // Tabs
  activateTab(tabId: string): Promise<void>
  closeTab(tabId: string): Promise<void>
  navigate(tabId: string, input: string): Promise<void>
  goBack(tabId?: string): Promise<void>
  goForward(tabId?: string): Promise<void>
  reload(tabId?: string): Promise<void>
  toggleMute(tabId: string): Promise<void>
  /** Puts a pinned tab back on its "home" URL. */
  resetPinned(tabId: string): Promise<void>
  /** Enters or leaves the tab's reader mode. */
  toggleReader(tabId: string): Promise<void>

  // Live folders
  /** Creates a live folder (`folderId` null) or changes its source; returns a translated error message, or null. */
  saveLiveFolder(folderId: string | null, source: LiveSource): Promise<string | null>
  refreshLiveFolder(folderId: string): Promise<void>
  /** True if a GitHub token is stored (the token itself never leaves main). */
  hasGithubToken(): Promise<boolean>
  /** Stores (encrypted) or removes (`null`) the GitHub token; returns a translated error message, or null. */
  setGithubToken(token: string | null): Promise<string | null>
  /** "New folder" button menu: plain folder or live folder. */
  showAddFolderMenu(): Promise<void>
  /** Translates the tab's page into the preferred language, or goes back to the original. */
  toggleTranslation(tabId: string): Promise<void>
  /**
   * Takes a capture of the tab (`capture` overlay). `copy`: returns the image (PNG as a data URL) that the UI puts on the
   * clipboard; `save`: closes the tool and asks where to save it. `ok` false if nothing was taken or saved.
   */
  takeCapture(tabId: string, target: CaptureTarget, action: 'copy' | 'save' | 'easel'): Promise<{ ok: boolean; png?: string }>
  /** Hover over a tab: preview next to `rect` (coordinates of the calling view); `below` = under the element. */
  showTabPreview(tabId: string, rect: Rect, below: boolean): Promise<void>
  hideTabPreview(): Promise<void>
  /** `#preview` view: card content, and its disappearance. */
  onTabPreview(cb: (p: TabPreviewPayload) => void): () => void
  onTabPreviewHide(cb: () => void): () => void

  // UI mods (Settings › Mods)
  listMods(): Promise<ModsInfo>
  /** Adds (null id) or edits a mod; returns its id. */
  saveMod(id: string | null, name: string, css: string): Promise<string>
  deleteMod(id: string): Promise<void>
  setModEnabled(id: string, enabled: boolean): Promise<void>
  /** Moves a mod one step up (-1) or down (1): the order is the order of application. */
  moveMod(id: string, delta: number): Promise<void>
  setModsDisabled(disabled: boolean): Promise<void>
  /** Imports a .css file (dialog); returns the id of the created mod, or null if cancelled. */
  importMod(): Promise<string | null>
  exportMod(id: string): Promise<void>
  /** Mods changed (elsewhere: View menu, another window): the section re-reads. */
  onModsChanged(cb: () => void): () => void
  showTabMenu(tabId: string): Promise<void>
  /** Moves a tab up (-1) or down (1) one rank in its list. */
  moveTabStep(tabId: string, dir: -1 | 1): Promise<void>
  /** "New" menu (tab, note, Easel), on right click on "New tab". */
  showNewMenu(): Promise<void>
  moveItem(source: DragSource, target: DropTarget): Promise<void>

  // Spaces
  switchSpace(spaceId: string): Promise<void>
  createSpace(draft: SpaceDraft): Promise<void>
  updateSpace(spaceId: string, draft: SpaceDraft): Promise<void>
  showSpaceMenu(spaceId: string): Promise<void>
  /** The main process asks the UI to open the space form. */
  onEditSpace(cb: (req: { spaceId: string | null }) => void): () => void

  // Sidebar
  toggleSidebar(): Promise<void>
  /** `live`: gesture in progress, main tolerates a slight overshoot (rubber-band) without persisting it. */
  setSidebarWidth(width: number, live?: boolean): Promise<void>
  clearToday(): Promise<void>
  /** Main announces a grouped archive of today's tabs (undoable with `undoArchive`). */
  onArchived(cb: (req: { id: number; kind: 'archived' | 'tabsClosed' | 'spaceDeleted'; count: number; name: string }) => void): () => void
  undoArchive(id: number): Promise<void>
  /** The undo window of announcement `id` has passed: it must close. */
  onUndoExpired(cb: (id: number) => void): () => void

  // Split view
  focusPane(tabId: string): Promise<void>
  unsplit(splitId: string): Promise<void>

  // Command bar
  /** `anchor`: address field from which the command bar grows (`edit-url` mode). */
  openPalette(mode: PaletteMode, anchor?: Rect): Promise<void>
  /** The overlay is subscribed to `overlay:show`: main can send it the pending payload. */
  overlayReady(): Promise<void>
  /** `instant`: click outside the panel, no exit animation (the view must stop swallowing clicks). */
  closeOverlay(instant?: boolean): Promise<void>
  /** The archive announcement view finished its exit: main removes it. */
  toastDone(): Promise<void>
  paletteQuery(query: string, mode: PaletteMode): Promise<PaletteResult[]>
  paletteSuggest(query: string): Promise<PaletteResult[]>
  paletteExecute(result: PaletteResult, mode: PaletteMode): Promise<void>
  onOverlayShow(cb: (p: OverlayPayload) => void): () => void
  /** `instant`: immediate unmount (no exit animation). */
  onOverlayHide(cb: (instant?: boolean) => void): () => void

  // Peek
  expandPeek(): Promise<void>

  // Tab row at the top (`horizontal` layout): popover above the pages, anchored under `anchor` (window coordinates)
  openPopover(popover: PopoverRequest, anchor: Rect): Promise<void>

  // Floating sidebar (when hidden)
  /** The mouse touches the window's left edge (true) or has left the floating sidebar (false). */
  setSidebarHover(hover: boolean): Promise<void>

  // Find in page (`#find` view)
  onFindState(cb: (s: FindState) => void): () => void
  /** Searches `query` in the active tab. `findNext` = go to the next occurrence of the same query. */
  find(query: string, opts: { forward: boolean; findNext: boolean }): Promise<void>
  closeFind(): Promise<void>

  // Downloads
  openDownload(id: string): Promise<void>
  revealDownload(id: string): Promise<void>
  cancelDownload(id: string): Promise<void>
  /** Resumes an interrupted download, or restarts its address if it cannot resume. */
  retryDownload(id: string): Promise<void>
  clearDownloads(): Promise<void>

  // Pinned folders
  /** Creates "New folder" at the head of the active space's pinned items and asks for its rename (`folder:rename`). */
  createFolder(): Promise<void>
  renameFolder(folderId: string, name: string): Promise<void>
  /** Toggles the folder, or sets it to the `open` state if given. */
  toggleFolder(folderId: string, open?: boolean): Promise<void>
  showFolderMenu(folderId: string): Promise<void>
  /** Main asks the UI to put the folder's name (or tab group's: same channel, `folderId` = group id) into edit mode. */
  onRenameFolder(cb: (req: { folderId: string }) => void): () => void

  // Today tab groups
  renameGroup(groupId: string, name: string): Promise<void>
  /** Collapses or expands the group, or sets it to the `open` state if given. */
  toggleGroup(groupId: string, open?: boolean): Promise<void>
  showGroupMenu(groupId: string): Promise<void>

  // Bookmarks (per space)
  /** Adds the tab's page to the space's bookmarks, or removes it if already there (⌘D, star). */
  toggleBookmark(tabId: string | null): Promise<void>
  /** Adds the tab to the active space's bookmarks at this spot (tab dropped on the bar). */
  addBookmark(tabId: string, target: BookmarkTarget): Promise<void>
  moveBookmark(id: string, target: BookmarkTarget): Promise<void>
  /** Opens the bookmark in tab `tabId` (the pane's), or in a new background tab (`newTab`). */
  openBookmark(id: string, tabId: string | null, newTab: boolean): Promise<void>
  renameBookmark(id: string, name: string): Promise<void>
  showBookmarkMenu(id: string, tabId: string | null): Promise<void>
  /** Right click on the bar outside a bookmark: new folder, hide the bar. */
  showBookmarksBarMenu(): Promise<void>
  /** Main asks the bookmarks bar to rename this bookmark (field under it). */
  onRenameBookmark(cb: (req: { id: string }) => void): () => void

  // Boosts (`#boost` window)
  onBoostState(cb: (s: BoostEditorState) => void): () => void
  /** Saves and applies right away to the site's open tabs. */
  saveBoost(site: string, patch: { css?: string; enabled?: boolean }): Promise<void>
  /** Starts the "Hide" tool in the site's active tab; each click adds a selector, Escape stops. */
  startZap(site: string): Promise<void>
  stopZap(): Promise<void>
  unzap(site: string, selector: string): Promise<void>
  deleteBoost(site: string): Promise<void>

  // Settings (`#settings` window)
  /** Opens Settings, on a section if given (`passwords`…). */
  openSettings(section?: string): Promise<void>
  /** UI error (React view), noted in the userData/logs/interface.log log. */
  reportUiError(route: string, message: string, stack: string): Promise<void>
  getSettingsInfo(): Promise<SettingsInfo>
  onSettingsInfo(cb: (info: SettingsInfo) => void): () => void
  updateSettings(patch: Partial<Settings>): Promise<void>
  /** First-launch introduction (`#welcome` window): reopens it (Settings › About). */
  openWelcome(): Promise<void>
  /** Help › Check for Updates (also in Settings › About): looks for a new version and shows the outcome. */
  checkForUpdates(): Promise<void>
  /** Finishes or skips the introduction: it will not come back on its own, the window closes. */
  welcomeFinish(): Promise<void>
  // Backup (Settings › Backup)
  getBackupInfo(): Promise<BackupInfo>
  /** Exports the checked parts to a chosen file; `password` required with passwords. Error message, or null. */
  exportBackup(sections: BackupSection[], password: string | null): Promise<string | null>
  /** Chooses a backup file: its summary, an error message, or null if cancelled. */
  openBackup(): Promise<BackupSummary | string | null>
  /** Opens a safety copy (by its name) like a chosen file. */
  openSafetyBackup(name: string): Promise<BackupSummary | string>
  /** Imports the checked parts then restarts; returns an error message, or null (cancelled). */
  applyBackup(token: string, sections: BackupSection[], mode: BackupMode, password: string | null): Promise<string | null>
  revealSafetyBackups(): Promise<void>
  /** Shortcut capture in progress (true) or done: menu shortcuts and ⌘Q are suspended during capture. */
  setShortcutRecording(on: boolean): Promise<void>
  setDefaultBrowser(): Promise<void>
  /** Chooses the downloads folder in a system dialog (cancelled = unchanged). */
  chooseDownloadDir(): Promise<void>
  /** Starts the import of a source (`importSources[].id`), for the checked data; the summary is shown in a dialog. */
  runImport(sourceId: string, kinds: ImportKind[]): Promise<void>
  openBoost(site: string): Promise<void>
  clearBrowsingData(kinds: BrowsingDataKind[]): Promise<void>
  /** Permission decisions remembered per site. */
  listSitePermissions(): Promise<SitePermissionEntry[]>
  /** Forgets one decision, those of a site, or all (no argument). */
  resetSitePermissions(origin?: string, permission?: string): Promise<void>
  /** Opens the extension store in a new tab. */
  openExtensionStore(): Promise<void>
  removeExtension(id: string): Promise<void>
  /** Unpacked extensions: choose a folder to load, reload, remove (the folder stays). */
  loadUnpackedExtension(): Promise<void>
  reloadUnpackedExtension(path: string): Promise<void>
  removeUnpackedExtension(path: string): Promise<void>
  /** Native context menu of an extension button (options, manage, remove…). */
  showExtensionMenu(id: string): Promise<void>
  /** Linux and Windows: opens the app menu (the hidden menu bar's) under the pointer. */
  showAppMenu(): Promise<void>
  /** Extensions button menu: all extensions with a button, and their pinning. Returns the one to open, or null. */
  showExtensionsMenu(): Promise<string | null>

  // Passwords (Settings › Passwords; never a password without `revealPassword`)
  listPasswords(): Promise<PasswordsInfo>
  /** Plaintext password, after Touch ID if available; null if refused. */
  revealPassword(id: string): Promise<string | null>
  /** Copies the password to the clipboard (Touch ID if available); false if refused. */
  copyPassword(id: string): Promise<boolean>
  /** Adds (null id) or edits a login; null `password` = unchanged. Returns an error message, or null. */
  savePassword(id: string | null, origin: string, username: string, password: string | null): Promise<string | null>
  deletePassword(id: string): Promise<void>
  /** Imports a CSV file chosen in a dialog; returns the report, or null if cancelled. */
  importPasswords(): Promise<PasswordImportResult | null>
  /** Exports as CSV (Touch ID then save dialog); false if cancelled. */
  exportPasswords(): Promise<boolean>
  /** Installs a password manager's extension from the store. */
  installPasswordExtension(manager: PasswordManager): Promise<void>
  /** Answer to the save prompt (overlay, `password` mode). */
  answerPasswordPrompt(token: string, answer: 'save' | 'never' | 'dismiss'): Promise<void>
  /** Key button of the address bar: reopens the pending prompt, otherwise lists the site's accounts, under `anchor`. */
  openPasswordKey(tabId: string, anchor: Rect): Promise<void>

  // Mode compact (lot A)
  toggleCompact(): Promise<void>
  /** The mouse touches the web card's top edge (true) or has left the floating toolbar (false). */
  setToolbarHover(hover: boolean): Promise<void>

  // Media (batch A)
  mediaToggle(tabId: string): Promise<void>
  mediaSkip(tabId: string, direction: 'next' | 'previous'): Promise<void>

  // Advanced split view (batch B)
  /** The sidebar starts dragging a tab: main shows the drop zones on the page. */
  beginTabDrag(tabId: string): Promise<void>
  endTabDrag(): Promise<void>
  /** Drop on a page zone: `center` replaces the displayed tab, the edges create or enlarge the split view. */
  dropOnContent(tabId: string, zone: 'left' | 'right' | 'top' | 'bottom' | 'center'): Promise<void>
  setSplitLayout(splitId: string, layout: SplitLayout): Promise<void>
  setSplitSizes(splitId: string, sizes: number[], live?: boolean): Promise<void>
  /** Developer tools of a tab: placement, close, size (0 = default; `done` at the end of the drag), button menu. */
  setDevToolsDock(tabId: string, dock: DevToolsDock): Promise<void>
  closeDevTools(tabId: string): Promise<void>
  setDevToolsSize(tabId: string, size: number, done: boolean): Promise<void>
  showDevToolsMenu(tabId: string): Promise<void>
  /** Menu of a tab's active development settings (address bar button). */
  showDevelopMenu(tabId: string): Promise<void>
  /** Responsive mode of a tab: device and size, or null to leave it. */
  setResponsive(tabId: string, responsive: DevResponsive | null): Promise<void>
  /** Dragging responsive mode handles: the fit scale is frozen while true, then recomputed. */
  setResponsiveDragging(tabId: string, dragging: boolean): Promise<void>
  /** Opens the tab's developer tools on the console. */
  showConsole(tabId: string): Promise<void>
  /** Detached tools window: state of its header, then its updates. */
  devToolsWindowReady(): Promise<DevToolsWindowState | null>
  onDevToolsWindowState(cb: (s: DevToolsWindowState) => void): () => void

  // Advanced spaces (batch C)
  setSpaceProfile(spaceId: string, profile: 'shared' | 'isolated'): Promise<void>
  addRoute(pattern: string, spaceId: string): Promise<void>
  removeRoute(routeId: string): Promise<void>

  // Library and tabs (batch D)
  openLibrary(section?: LibrarySection): Promise<void>
  /** Opens a history entry in a new tab of the main window. */
  openLibraryUrl(url: string): Promise<void>
  onLibrarySection(cb: (section: LibrarySection) => void): () => void
  queryHistory(query: string, offset: number, limit: number): Promise<HistoryEntry[]>
  deleteHistory(url: string): Promise<void>
  listArchive(query: string): Promise<ArchivedEntry[]>
  restoreArchived(id: string): Promise<void>
  deleteArchived(id: string): Promise<void>
  renameTab(tabId: string, title: string | null): Promise<void>
  /** Emoji of a tab, folder or group; `null` = default icon. */
  setIcon(target: IconTarget, emoji: string | null): Promise<void>
  /** Main asks the sidebar to put a tab's title into edit mode. */
  onRenameTab(cb: (req: { tabId: string }) => void): () => void
  /** Main asks the UI to open the emoji picker of a tab, folder or group. */
  onPickIcon(cb: (target: IconTarget) => void): () => void
  /** Opens the active tab's site panel, anchored under `anchor` (window coordinates). */
  openSitePanel(anchor: Rect): Promise<void>
  setSitePermission(origin: string, permission: string, state: PermissionState): Promise<void>
  clearSiteData(origin: string): Promise<void>

  // Devices (link with the phone: Settings › Devices, toolbar indicator)
  getLinkState(): Promise<LinkState>
  onLinkState(cb: (s: LinkState) => void): () => void
  /** Master switch ("Sync"). */
  setLinkGeneral(on: boolean): Promise<void>
  /** Switch for one phone, on this computer. */
  setLinkDevice(linkId: string, on: boolean): Promise<void>
  /** Removes the link (keys included) and notifies the phone if connected. */
  forgetLink(linkId: string): Promise<void>
  /** Creates a pairing QR code (replaces the previous one); the pushed state contains it. False if the computer cannot listen. */
  startLinkPairing(): Promise<boolean>
  cancelLinkPairing(): Promise<void>

  // UI language (all views)
  getLocale(): Promise<Locale>
  onLocale(cb: (locale: Locale) => void): () => void
  // UI appearance (all views)
  getAppearance(): Promise<Appearance>
  onAppearance(cb: (a: Appearance) => void): () => void
  /** Favicon the UI could not load, fetched by main (data URL), or null. */
  fallbackFavicon(url: string): Promise<string | null>

  // Confirmation de sortie (vue `#quit`)
  onQuitState(cb: (s: QuitHudState) => void): () => void

  // Floating mini player (`#pip` view)
  onPipState(cb: (s: PipState) => void): () => void
  /** `seek` : `value` = position en secondes ; `back` / `forward` : 15 s. */
  pipCommand(command: PipCommand, value?: number): Promise<void>
  /** Opens the mini-player for the active tab's video (context menu, command bar). */
  openPip(tabId?: string): Promise<void>

  // Little Signet (called from a Little Signet window's bar)
  onLittleState(cb: (s: LittleState) => void): () => void
  /** The bar is subscribed to `little:state`: main sends the state back. */
  littleReady(): Promise<void>
  littleOpenInMain(): Promise<void>
  littleCopyUrl(): Promise<void>

  // Private windows (called from a private window's bar, which targets its own window)
  getPrivateState(): Promise<PrivateState | null>
  onPrivateState(cb: (s: PrivateState) => void): () => void
  onPrivateFocusAddress(cb: () => void): () => void
  privateNewTab(): Promise<void>
  privateActivate(tabId: string): Promise<void>
  privateCloseTab(tabId: string): Promise<void>
  privateNavigate(input: string): Promise<void>
  openPrivateWindow(): Promise<void>

  // Apps (installed sites: `webapp` overlay, Settings › Apps)
  /** Installs the site offered by the overlay (`token`) under this name, with this icon (1024 px PNG as a data URL). */
  installWebApp(token: string, name: string, icon: string): Promise<void>
  listWebApps(): Promise<WebAppsInfo>
  openWebApp(id: string): Promise<void>
  renameWebApp(id: string, name: string): Promise<void>
  /** Shows the launcher in Finder (recreated if it disappeared). */
  revealWebApp(id: string): Promise<void>
  removeWebApp(id: string): Promise<void>
  /** An app was installed, renamed or deleted: the section re-reads. */
  onWebAppsChanged(cb: () => void): () => void

  // Bar of an app window (`#webapp` route, which targets its own window)
  onWebAppState(cb: (s: WebAppState) => void): () => void
  /** The bar is subscribed to `webapp:state`: main sends the state back. */
  webAppReady(): Promise<void>
  /** Goes back to the app's start address. */
  webAppHome(): Promise<void>
  /** Opens the page in a Signet tab and closes the app window. */
  webAppOpenInMain(): Promise<void>
  webAppCopyUrl(): Promise<void>
}

declare global {
  interface Window {
    signet: SignetApi
  }
}

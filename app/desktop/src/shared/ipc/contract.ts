// IPC contract: single source of truth for UI → main calls and main → UI messages.
// For each `SignetApi` method: its channel, its kind (`invoke` call or `event` subscription) and, for a call,
// the schema of its arguments (checked by main before calling the handler).
// Pure TypeScript, no Node or Electron import: the sandboxed preload bundles it.
import type { SignetApi, Settings } from '../types'
import {
  any,
  arr,
  backupMode,
  backupSection,
  bookmarkTarget,
  bool,
  browsingDataKind,
  captureTarget,
  devResponsive,
  devToolsDock,
  dragSource,
  dropTarget,
  iconTarget,
  importKind,
  librarySection,
  liveSource,
  lit,
  nullable,
  num,
  obj,
  opt,
  paletteMode,
  paletteResult,
  passwordManager,
  permissionState,
  pipCommand,
  plain,
  popoverRequest,
  rect,
  spaceDraft,
  splitLayout,
  str,
  type Guard
} from './validate'

/** Call from the UI to main (`ipcRenderer.invoke` ↔ `ipcMain.handle`). */
export interface InvokeDef<A extends unknown[] = unknown[]> {
  readonly kind: 'invoke'
  readonly channel: string
  readonly args: { readonly [I in keyof A]-?: Guard<A[I]> }
}

/** Subscription to a message pushed by main (`webContents.send` ↔ `ipcRenderer.on`). */
export interface EventDef {
  readonly kind: 'event'
  readonly channel: string
}

/** Fire-and-forget send from the UI to main (`ipcRenderer.send` ↔ `ipcMain.on`). */
export interface SendDef<A extends unknown[] = unknown[]> {
  readonly kind: 'send'
  readonly channel: string
  readonly args: { readonly [I in keyof A]-?: Guard<A[I]> }
}

const invoke = <const C extends string, const G extends readonly Guard<unknown>[]>(channel: C, args: G) => ({ kind: 'invoke', channel, args }) as const
const event = <const C extends string>(channel: C) => ({ kind: 'event', channel }) as const
const send = <const C extends string, const G extends readonly Guard<unknown>[]>(channel: C, args: G) => ({ kind: 'send', channel, args }) as const

// Shortcuts for the most frequent arguments.
const id = str()
const optId = opt(str())
const idOrNull = nullable(str())

/**
 * Checks an entry against the `SignetApi` method's signature: a method returning a promise is a call
 * (arguments = parameters), any other is a subscription (`onX(cb)`). Guards must produce the parameter's type.
 */
type EntryFor<F> = F extends (...a: infer A) => Promise<unknown> ? InvokeDef<A> : F extends (cb: never) => () => void ? EventDef : never

/** All of `SignetApi`, method by method. A method added to `SignetApi` without an entry here (or the reverse) does not compile. */
export const IPC = {
  // State, language, appearance
  getState: invoke('state:get', []),
  onState: event('state'),
  onStatePatch: event('state:patch'),
  onPendingSpace: event('space:pending'),
  onEditAddressRequest: event('palette:edit-url-request'),
  getLocale: invoke('locale:get', []),
  onLocale: event('locale'),
  getAppearance: invoke('appearance:get', []),
  onAppearance: event('appearance'),
  fallbackFavicon: invoke('favicon:fallback', [id]),

  // Tabs
  activateTab: invoke('tab:activate', [id]),
  closeTab: invoke('tab:close', [id]),
  navigate: invoke('tab:navigate', [id, str()]),
  goBack: invoke('tab:back', [optId]),
  goForward: invoke('tab:forward', [optId]),
  reload: invoke('tab:reload', [optId]),
  toggleMute: invoke('tab:toggle-mute', [id]),
  resetPinned: invoke('tab:reset-pinned', [id]),
  toggleReader: invoke('tab:reader', [id]),
  toggleTranslation: invoke('tab:translate', [id]),
  // A malformed capture returns `{ ok: false }` (see ipc/tabs.ts), as before.
  takeCapture: invoke('capture:take', [id, captureTarget, lit('copy', 'save', 'easel')]),
  showTabPreview: invoke('preview:show', [id, rect, bool()]),
  hideTabPreview: invoke('preview:hide', []),
  onTabPreview: event('preview:show'),
  onTabPreviewHide: event('preview:hide'),
  showTabMenu: invoke('tab:menu', [id]),
  moveTabStep: invoke('tab:move-step', [id, lit(-1, 1)]),
  showNewMenu: invoke('notes:new-menu', []),
  moveItem: invoke('item:move', [dragSource, dropTarget]),
  renameTab: invoke('tab:rename', [id, idOrNull]),
  setIcon: invoke('icon:set', [iconTarget, idOrNull]),
  onRenameTab: event('tab:rename-request'),
  onPickIcon: event('icon:request'),

  // Live folders
  saveLiveFolder: invoke('live:save', [idOrNull, liveSource]),
  refreshLiveFolder: invoke('live:refresh', [id]),
  hasGithubToken: invoke('live:token-status', []),
  setGithubToken: invoke('live:set-token', [idOrNull]),
  showAddFolderMenu: invoke('folder:add-menu', []),

  // Interface mods
  listMods: invoke('mods:list', []),
  saveMod: invoke('mods:save', [idOrNull, str(), str()]),
  deleteMod: invoke('mods:delete', [id]),
  setModEnabled: invoke('mods:enable', [id, bool()]),
  moveMod: invoke('mods:move', [id, num()]),
  setModsDisabled: invoke('mods:disable-all', [bool()]),
  importMod: invoke('mods:import', []),
  exportMod: invoke('mods:export', [id]),
  onModsChanged: event('mods:changed'),

  // Spaces
  switchSpace: invoke('space:switch', [id]),
  createSpace: invoke('space:create', [spaceDraft]),
  updateSpace: invoke('space:update', [id, spaceDraft]),
  showSpaceMenu: invoke('space:menu', [id]),
  setSpaceProfile: invoke('space:profile', [id, lit('shared', 'isolated')]),
  addRoute: invoke('route:add', [str(), id]),
  removeRoute: invoke('route:remove', [id]),
  onEditSpace: event('space:edit'),

  // Folders, groups
  createFolder: invoke('folder:create', []),
  renameFolder: invoke('folder:rename', [id, str()]),
  toggleFolder: invoke('folder:toggle', [id, opt(bool())]),
  showFolderMenu: invoke('folder:menu', [id]),
  onRenameFolder: event('folder:rename'),
  renameGroup: invoke('group:rename', [id, str()]),
  toggleGroup: invoke('group:toggle', [id, opt(bool())]),
  showGroupMenu: invoke('group:menu', [id]),

  // Bookmarks
  toggleBookmark: invoke('bookmark:toggle', [idOrNull]),
  addBookmark: invoke('bookmark:add', [id, bookmarkTarget]),
  moveBookmark: invoke('bookmark:move', [id, bookmarkTarget]),
  openBookmark: invoke('bookmark:open', [id, idOrNull, bool()]),
  renameBookmark: invoke('bookmark:rename', [id, str()]),
  showBookmarkMenu: invoke('bookmark:menu', [id, idOrNull]),
  showBookmarksBarMenu: invoke('bookmarks:menu', []),
  onRenameBookmark: event('bookmark:rename'),

  // Sidebar, archive
  toggleSidebar: invoke('sidebar:toggle', []),
  setSidebarWidth: invoke('sidebar:width', [num(), opt(bool())]),
  setSidebarHover: invoke('sidebar:hover', [bool()]),
  clearToday: invoke('today:clear', []),
  onArchived: event('today:archived'),
  undoArchive: invoke('today:undo', [num()]),
  onUndoExpired: event('today:undo-expired'),

  // Split view
  focusPane: invoke('pane:focus', [id]),
  unsplit: invoke('split:remove', [id]),
  setSplitLayout: invoke('split:layout', [id, splitLayout]),
  setSplitSizes: invoke('split:sizes', [id, arr(num()), opt(bool())]),
  beginTabDrag: invoke('drag:begin', [id]),
  endTabDrag: invoke('drag:end', []),
  dropOnContent: invoke('drag:drop', [id, lit('left', 'right', 'top', 'bottom', 'center')]),

  // Developer tools
  setDevToolsDock: invoke('devtools:dock', [id, devToolsDock]),
  closeDevTools: invoke('devtools:close', [id]),
  setDevToolsSize: invoke('devtools:size', [id, num(), bool()]),
  showDevToolsMenu: invoke('devtools:menu', [id]),
  showConsole: invoke('devtools:console', [id]),
  showDevelopMenu: invoke('develop:menu', [id]),
  // Responsive mode: plain object only, `Develop.setResponsive` revalidates the device and sizes.
  setResponsive: invoke('develop:responsive', [id, nullable(devResponsive)]),
  setResponsiveDragging: invoke('develop:responsive-drag', [id, bool()]),
  devToolsWindowReady: invoke('devtools:window-ready', []),
  onDevToolsWindowState: event('devtools:window-state'),

  // Devices
  getLinkState: invoke('link:get', []),
  onLinkState: event('link:state'),
  setLinkGeneral: invoke('link:general', [bool()]),
  setLinkDevice: invoke('link:device', [id, bool()]),
  forgetLink: invoke('link:forget', [id]),
  startLinkPairing: invoke('link:pair', []),
  cancelLinkPairing: invoke('link:pair-cancel', []),

  // Command bar, overlay, Peek
  openPalette: invoke('palette:open', [paletteMode, opt(rect)]),
  openPopover: invoke('popover:open', [popoverRequest, rect]),
  overlayReady: invoke('overlay:ready', []),
  closeOverlay: invoke('overlay:close', [opt(bool())]),
  toastDone: invoke('toast:done', []),
  paletteQuery: invoke('palette:query', [str(), paletteMode]),
  paletteSuggest: invoke('palette:suggest', [str()]),
  paletteExecute: invoke('palette:execute', [paletteResult, paletteMode]),
  onOverlayShow: event('overlay:show'),
  onOverlayHide: event('overlay:hide'),
  expandPeek: invoke('peek:expand', []),

  // Compact mode, media
  toggleCompact: invoke('compact:toggle', []),
  setToolbarHover: invoke('toolbar:hover', [bool()]),
  mediaToggle: invoke('media:toggle', [id]),
  mediaSkip: invoke('media:skip', [id, lit('next', 'previous')]),

  // Quit confirmation, find in page
  onQuitState: event('quit:state'),
  onFindState: event('find:state'),
  find: invoke('find:query', [str(), obj({ forward: bool(), findNext: bool() })]),
  closeFind: invoke('find:close', []),

  // Downloads
  openDownload: invoke('download:open', [id]),
  revealDownload: invoke('download:reveal', [id]),
  cancelDownload: invoke('download:cancel', [id]),
  retryDownload: invoke('download:retry', [id]),
  clearDownloads: invoke('downloads:clear', []),

  // Boosts
  onBoostState: event('boost:state'),
  saveBoost: invoke('boost:save', [str(), obj({ css: opt(str()), enabled: opt(bool()) })]),
  startZap: invoke('boost:zap', [str()]),
  stopZap: invoke('boost:stop-zap', []),
  unzap: invoke('boost:unzap', [str(), str()]),
  deleteBoost: invoke('boost:delete', [str()]),

  // Settings
  openSettings: invoke('settings:open', [optId]),
  // Permissive: an error report must not fail in turn; `logUiError` converts everything to a string.
  reportUiError: invoke('ui:error', [any<string>(), any<string>(), any<string>()]),
  getSettingsInfo: invoke('settings:get', []),
  onSettingsInfo: event('settings:info'),
  // Plain object: `cleanSettings` and the `Settings.update` checks sanitize each field.
  updateSettings: invoke('settings:update', [plain<Partial<Settings>>()]),
  setShortcutRecording: invoke('shortcuts:recording', [bool()]),
  setDefaultBrowser: invoke('settings:default-browser', []),
  chooseDownloadDir: invoke('settings:download-dir', []),
  runImport: invoke('settings:import', [str(), arr(importKind)]),
  openWelcome: invoke('welcome:open', []),
  checkForUpdates: invoke('updates:check', []),
  welcomeFinish: invoke('welcome:finish', []),
  openBoost: invoke('settings:open-boost', [str()]),
  clearBrowsingData: invoke('settings:clear-data', [arr(browsingDataKind)]),
  listSitePermissions: invoke('permissions:list', []),
  resetSitePermissions: invoke('permissions:reset', [optId, optId]),

  // Backup
  getBackupInfo: invoke('backup:info', []),
  exportBackup: invoke('backup:export', [arr(backupSection), idOrNull]),
  openBackup: invoke('backup:open', []),
  openSafetyBackup: invoke('backup:open-safety', [str()]),
  applyBackup: invoke('backup:apply', [str(), arr(backupSection), backupMode, idOrNull]),
  revealSafetyBackups: invoke('backup:reveal-safety', []),

  // Extensions, app menus
  openExtensionStore: invoke('settings:open-extension-store', []),
  removeExtension: invoke('settings:remove-extension', [id]),
  loadUnpackedExtension: invoke('extensions:load-unpacked', []),
  reloadUnpackedExtension: invoke('extensions:reload-unpacked', [str()]),
  removeUnpackedExtension: invoke('extensions:remove-unpacked', [str()]),
  showExtensionMenu: invoke('extension:menu', [id]),
  showExtensionsMenu: invoke('extension:list-menu', []),
  showAppMenu: invoke('app:menu', []),

  // Passwords
  listPasswords: invoke('passwords:list', []),
  revealPassword: invoke('passwords:reveal', [id]),
  copyPassword: invoke('passwords:copy', [id]),
  savePassword: invoke('passwords:save', [idOrNull, str(), str(), idOrNull]),
  deletePassword: invoke('passwords:delete', [id]),
  importPasswords: invoke('passwords:import', []),
  exportPasswords: invoke('passwords:export', []),
  installPasswordExtension: invoke('passwords:install-extension', [passwordManager]),
  answerPasswordPrompt: invoke('passwords:prompt', [id, lit('save', 'never', 'dismiss')]),
  openPasswordKey: invoke('passwords:key', [id, rect]),

  // Library, site panel
  openLibrary: invoke('library:open', [opt(librarySection)]),
  openLibraryUrl: invoke('library:open-url', [str()]),
  onLibrarySection: event('library:section'),
  queryHistory: invoke('history:query', [str(), num(), num()]),
  deleteHistory: invoke('history:delete', [str()]),
  listArchive: invoke('archive:list', [str()]),
  restoreArchived: invoke('archive:restore', [str()]),
  deleteArchived: invoke('archive:delete', [str()]),
  openSitePanel: invoke('site:panel', [rect]),
  setSitePermission: invoke('site:permission', [str(), str(), permissionState]),
  clearSiteData: invoke('site:clear', [str()]),

  // Mini player
  onPipState: event('pip:state'),
  pipCommand: invoke('pip:command', [pipCommand, opt(num())]),
  openPip: invoke('pip:open', [optId]),

  // Little Signet
  onLittleState: event('little:state'),
  littleReady: invoke('little:ready', []),
  littleOpenInMain: invoke('little:open-in-main', []),
  littleCopyUrl: invoke('little:copy-url', []),

  // Private windows
  getPrivateState: invoke('private:get', []),
  onPrivateState: event('private:state'),
  onPrivateFocusAddress: event('private:focus-address'),
  privateNewTab: invoke('private:new-tab', []),
  privateActivate: invoke('private:activate', [id]),
  privateCloseTab: invoke('private:close', [id]),
  privateNavigate: invoke('private:navigate', [str()]),
  openPrivateWindow: invoke('private:open', []),

  // Passkey from a phone
  getPasskeyPrompt: invoke('passkey:get', []),
  cancelPasskeyPrompt: invoke('passkey:cancel', []),

  // Apps
  installWebApp: invoke('webapp:install', [str(), str(), str()]),
  listWebApps: invoke('webapps:list', []),
  openWebApp: invoke('webapps:open', [id]),
  renameWebApp: invoke('webapps:rename', [id, str()]),
  revealWebApp: invoke('webapps:reveal', [id]),
  removeWebApp: invoke('webapps:remove', [id]),
  onWebAppsChanged: event('webapps:changed'),
  onWebAppState: event('webapp:state'),
  webAppReady: invoke('webapp:ready', []),
  webAppHome: invoke('webapp:home', []),
  webAppOpenInMain: invoke('webapp:open-in-main', []),
  webAppCopyUrl: invoke('webapp:copy-url', [])
} as const satisfies { readonly [K in keyof SignetApi]: EntryFor<SignetApi[K]> }

export type IpcContract = typeof IPC

/** `SignetApi` methods that are calls (a promise returned) or subscriptions. */
export type InvokeMethod = { [K in keyof SignetApi]: ReturnType<SignetApi[K]> extends Promise<unknown> ? K : never }[keyof SignetApi]
export type EventMethod = Exclude<keyof SignetApi, InvokeMethod>

/** Messages pushed by main to UI views: channel → `webContents.send` arguments. Derived from `SignetApi`. */
export type UiEventMap = {
  [K in EventMethod as IpcContract[K]['channel']]: SignetApi[K] extends (cb: (...p: infer P) => void) => () => void ? P : never
}

/** Channel of an event or a call, by method name. */
export const channelOf = (method: keyof SignetApi): string => IPC[method].channel

// ───────────────────────── Mini player (`window.signetPip`) ─────────────────────────

/** Mini-player gestures: fire-and-forget sends, outside `SignetApi` (`#pip` route only). */
export interface SignetPipApi {
  drag(on: boolean): void
  /** Manual resize: absolute pointer movement since the start of the gesture. */
  resize(phase: 'start' | 'move' | 'end', handle?: string, dx?: number, dy?: number): void
}

export const PIP_IPC = {
  drag: send('pip:drag', [bool()]),
  resize: send('pip:resize', [lit('start', 'move', 'end'), opt(str()), opt(num()), opt(num())])
} as const satisfies { readonly [K in keyof SignetPipApi]: SendDef<Parameters<SignetPipApi[K]>> }

// ───────────────────────── Web page channels (UNTRUSTED) ─────────────────────────

/**
 * Channels called by page scripts (src/preload/swipe.ts, passwords.ts, webauthn.ts, notes.ts), hence from tabs:
 * never by the UI. Each has its own sender check in main (see ipc/page.ts, passwords.ts,
 * notes.ts); they do not go through the "UI only" guard nor the schemas above.
 */
export const PAGE_IPC = {
  'swipe:can': 'invoke',
  'swipe:navigate': 'send',
  'page:submit': 'send',
  'pw:accounts': 'invoke',
  'pw:fill': 'invoke',
  'pw:submit': 'send',
  'webauthn:request': 'invoke',
  'webauthn:abort': 'send',
  'notes:locale': 'invoke',
  'notes:appearance': 'invoke',
  'notes:load': 'invoke',
  'notes:save': 'invoke',
  'notes:remove': 'invoke'
} as const satisfies Record<string, 'invoke' | 'send'>

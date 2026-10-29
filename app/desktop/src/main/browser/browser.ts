import type { BrowserWindow } from 'electron'
import { newId } from '@signet/shared/ids'
import { Adblock } from '../web/adblock'
import { Permissions } from '../web/permissions'
import { initProfiles } from '../web/profiles'
import { Boosts } from '../features/boost/boost'
import { Compact } from './compact'
import { Downloads } from '../web/downloads'
import { FindBar } from './find'
import type { History } from '../core/history'
import { LibraryWindow } from '../windows/library'
import { LittleWindows } from '../windows/little'
import { Media } from './media'
import { Pip } from '../windows/pip'
import { PrivateWindows } from '../windows/private'
import { SettingsWindow } from '../windows/settings'
import { WelcomeWindow } from '../windows/welcome'
import { PasskeyPrompt } from '../windows/passkey'
import { Swipe } from './swipe'
import { Bookmarks } from '../features/bookmarks'
import { SitePanel } from './sitepanel'
import { PageTools } from '../features/pagetools'
import { Capture } from '../features/capture'
import { Backup } from '../features/backup'
import { Notes } from '../features/notes/notes'
import { LiveFolders } from '../features/livefolders/livefolders'
import { TabPreviews } from './preview'
import { Mods } from '../features/mods/mods'
import { Reader } from '../features/reader'
import { Translator } from '../features/translate'
import { Passwords } from '../features/passwords/passwords'
import { WebApps } from '../features/webapps'
import { Link } from '../features/link/link'
import { Updates } from '../features/updates'
import type { Store } from '../core/store'
import { createTabView } from './tabs'
import { Extensions } from '../features/extensions/extensions'
import { SiteColor } from './sitecolor'
import { openShortcut } from '../web/internal'
import { t } from '../app/i18n'
import { DevTools } from '../devtools/devtools'
import { DevMode } from '../devtools/devmode'
import { Develop } from '../devtools/develop'
import { JsonView } from '../devtools/jsonview'
import { UnpackedExtensions } from '../features/extensions/unpacked'
import { buildAppMenu, menuKey } from '../app/menu'
import { Model } from '../model'
import { Arrange } from './arrange'
import { Broadcast, type Changed } from './broadcast'
import { Floating } from './floating'
import { FolderActions } from './folder-actions'
import { GroupActions } from './group-actions'
import { HtmlFullscreen } from './fullscreen'
import { Layout } from './layout'
import { Overlay } from './overlay'
import { Peek } from './peek'
import { Shell } from './shell'
import { TabSleep } from './sleep'
import { SpaceActions } from './space-actions'
import { HttpsOnly } from './https-only'
import { PageZoom } from './page-zoom'
import { SplitView } from './splitview'
import { TabActions } from './tab-actions'
import { TabDrag } from './drag'
import { Undo } from './undo'
import { Views } from './views'
import { MainWindow } from './window'

/**
 * Composition root: creates the browser's parts and links them. It holds no logic; see `model/`
 * (state rules), `views.ts` (pages), `layout.ts` + `broadcast.ts` (window and broadcast), `overlay.ts` + `peek.ts`
 * (overlays), and the `*-actions.ts` (user gestures).
 *
 * A few accessors are kept here because all the code uses them: `store`, `history`, `win` (the main window)
 * and `space` (the active space).
 */
export class Browser {
  win: BrowserWindow | null = null

  // Functions (each keeps a reference to the browser, but only uses it when called)
  readonly little = new LittleWindows(this)
  readonly privates = new PrivateWindows(this)
  readonly find = new FindBar(this)
  readonly downloads = new Downloads(this)
  readonly boosts = new Boosts(this)
  readonly settings = new SettingsWindow(this)
  readonly welcome = new WelcomeWindow(this)
  readonly passkeys = new PasskeyPrompt(this)
  readonly swipe = new Swipe(this)
  readonly bookmarks = new Bookmarks(this)
  readonly library = new LibraryWindow(this)
  readonly sitePanel = new SitePanel(this)
  readonly compact = new Compact(this)
  readonly siteColor = new SiteColor(this)
  readonly media = new Media(this)
  readonly pip = new Pip(this)
  readonly pageTools = new PageTools(this)
  readonly capture = new Capture(this)
  readonly backup = new Backup(this)
  readonly notes = new Notes(this)
  readonly liveFolders = new LiveFolders(this)
  readonly previews = new TabPreviews(this)
  readonly devtools = new DevTools(this)
  readonly devMode = new DevMode(this)
  readonly develop = new Develop(this)
  readonly jsonView = new JsonView(this)
  readonly unpacked = new UnpackedExtensions(this)
  readonly mods = new Mods(this)
  readonly reader = new Reader(this)
  readonly translator = new Translator(this)
  readonly adblock: Adblock
  readonly extensions = new Extensions(this)
  readonly passwords = new Passwords(this)
  readonly webApps = new WebApps(this)
  readonly link = new Link(this)
  readonly updates = new Updates(this)
  readonly permissions: Permissions
  readonly httpsOnly: HttpsOnly
  readonly pageZoom: PageZoom

  // Core: the state rules, then the pages, the broadcast and the layout
  readonly model: Model
  readonly views: Views
  readonly sleep: TabSleep
  readonly floating: Floating
  readonly layout: Layout
  readonly fullscreen: HtmlFullscreen
  readonly undo: Undo
  readonly overlay: Overlay
  readonly peek: Peek
  readonly broadcast: Broadcast
  readonly window: MainWindow

  // User gestures
  readonly tabs: TabActions
  readonly arrange: Arrange
  readonly folders: FolderActions
  readonly groups: GroupActions
  readonly splitView: SplitView
  readonly drag: TabDrag
  readonly spaces: SpaceActions
  readonly shell: Shell

  constructor(
    readonly store: Store,
    readonly history: History
  ) {
    this.adblock = new Adblock(store)
    this.permissions = new Permissions(store)
    this.httpsOnly = new HttpsOnly({ store })
    // The parts use one another: these two shortcuts only read `this.broadcast` when called.
    const changed: Changed = (opts) => this.broadcast.changed(opts)
    const apply = () => this.broadcast.apply()

    this.model = new Model(store, {
      newId,
      now: () => Date.now(),
      defaultFolderName: () => t('defaults.folder'),
      defaultSpaceName: () => t('defaults.space'),
      onActivated: (space, tabId) => {
        this.liveFolders.onActivated(tabId)
        if (space.id === store.state.activeSpaceId) this.layout.requestFocus()
      },
      destroyView: (tabId) => this.views.destroyView(tabId),
      saveHistory: (tabId) => this.views.saveHistory(tabId),
      reconcileSessions: (tabIds) => this.views.reconcileSessions(tabIds)
    })
    this.views = new Views({
      store,
      window: this,
      create: (tabId, ses) => createTabView(this, tabId, ses),
      upgradeUrl: (tabId, ses, url) => this.httpsOnly.resolve(tabId, ses, url),
      extensions: this.extensions,
      changed: () => changed()
    })
    this.pageZoom = new PageZoom({ store, views: this.views, persistent: (ses) => !this.privates.hasSession(ses) })
    // Intended order: the mini player renders the page before sleep forgets the tab.
    this.views.onGone((tabId) => this.pip.onGone(tabId))
    this.sleep = new TabSleep({
      store,
      window: this,
      views: this.views,
      pip: this.pip,
      devtools: this.devtools,
      media: this.media,
      permissions: this.permissions,
      shown: () => this.layout.shownTabs,
      peekTabId: () => this.peek.tabId,
      paused: () => this.overlay.kind === 'dropzones',
      changed
    })
    this.floating = new Floating({ store, window: this, changed, overlayOpen: () => this.overlay.open })
    this.fullscreen = new HtmlFullscreen({
      window: this,
      shown: () => this.layout.shownTabs,
      inMiniPlayer: (tabId) => this.pip.owns(tabId),
      contentsOf: (tabId) => this.views.viewOf(tabId)?.webContents,
      changed
    })
    this.fullscreen.onOther((tabId) => {
      this.privates.refit(tabId)
      this.webApps.refit(tabId)
      this.little.refit(tabId)
    })
    this.layout = new Layout({
      store,
      window: this,
      changed,
      views: this.views,
      sleep: this.sleep,
      floating: this.floating,
      compact: this.compact,
      find: this.find,
      pip: this.pip,
      devtools: this.devtools,
      develop: this.develop,
      bookmarks: this.bookmarks,
      extensions: this.extensions,
      media: this.media,
      permissions: this.permissions,
      siteColor: this.siteColor,
      previews: this.previews,
      fullscreen: this.fullscreen
    })
    this.undo = new Undo({ store, window: this, changed })
    this.overlay = new Overlay({
      store,
      window: this,
      layout: this.layout,
      floating: this.floating,
      compact: this.compact,
      passwords: this.passwords,
      apply,
      changed
    })
    this.peek = new Peek({
      store,
      window: this,
      views: this.views,
      layout: this.layout,
      floating: this.floating,
      overlay: this.overlay,
      apply,
      changed,
      adopt: (tabId) => this.tabs.adopt(tabId)
    })
    this.overlay.attach(this.peek)
    this.layout.attach({ overlay: this.overlay, peek: this.peek, toast: this.undo })
    this.broadcast = new Broadcast({
      store,
      window: this,
      layout: this.layout,
      floating: this.floating,
      compact: this.compact,
      media: this.media,
      previews: this.previews,
      overlay: this.overlay,
      menu: { key: () => menuKey(this), rebuild: () => buildAppMenu(this) },
      satellites: (snapshot) => {
        this.little.pushStates()
        this.webApps.pushStates()
        this.privates.sync()
        this.boosts.push()
        this.settings.push()
        this.library.push(snapshot)
      }
    })

    this.tabs = new TabActions({
      store,
      model: this.model,
      views: this.views,
      httpsOnly: this.httpsOnly,
      pageZoom: this.pageZoom,
      changed,
      undo: this.undo,
      overlay: this.overlay,
      peek: this.peek,
      privates: this.privates,
      webApps: this.webApps,
      little: this.little,
      showMain: () => this.window.show(),
      openShortcut: (url) => openShortcut(this, url),
      openInSplit: (url) => this.splitView.openInSplit(url),
      busy: (tabId) => this.pip.owns(tabId) || this.media.isActive(tabId) || this.devtools.isOpen(tabId)
    })
    this.arrange = new Arrange({ model: this.model, changed })
    this.folders = new FolderActions({
      store,
      window: this,
      model: this.model,
      changed,
      floating: this.floating,
      overlay: this.overlay,
      undo: this.undo
    })
    this.groups = new GroupActions({ store, model: this.model, changed, undo: this.undo })
    this.splitView = new SplitView({ model: this.model, changed, tabs: this.tabs })
    this.drag = new TabDrag({
      store,
      window: this,
      model: this.model,
      changed,
      apply,
      peek: this.peek,
      floating: this.floating,
      overlay: this.overlay,
      layout: this.layout,
      tabs: this.tabs
    })
    this.spaces = new SpaceActions({
      store,
      window: this,
      model: this.model,
      changed,
      layout: this.layout,
      overlay: this.overlay,
      undo: this.undo,
      views: this.views
    })
    this.shell = new Shell({
      store,
      changed,
      floating: this.floating,
      compact: this.compact,
      overlay: this.overlay,
      layout: this.layout,
      views: this.views,
      develop: this.develop,
      adblock: this.adblock
    })
    this.window = new MainWindow({
      store,
      host: this,
      layout: this.layout,
      fullscreen: this.fullscreen,
      overlay: this.overlay,
      compact: this.compact,
      find: this.find,
      floating: this.floating,
      sleep: this.sleep,
      views: this.views,
      peek: this.peek,
      undo: this.undo,
      previews: this.previews,
      media: this.media,
      develop: this.develop,
      changed,
      stepSpace: (step) => this.spaces.step(step)
    })

    this.permissions.setVisibility((wc) => {
      const id = this.views.tabIdOfContents(wc.id)
      // Private tab in the background of its window: the request waits until it is shown.
      return !id || (store.detached.has(id) && !this.privates.hidden(id)) || this.layout.shownTabs.includes(id)
    })
    initProfiles(this)
  }

  /** Active space. */
  get space() {
    return this.model.space
  }
}

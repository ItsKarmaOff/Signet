import { app, BrowserWindow, dialog, Menu, webContents, type MenuItemConstructorOptions } from 'electron'
import type { SplitLayout } from '@shared/types'
import type { Browser } from '../browser/browser'
import { renameTab, setTabIcon } from '../windows/library'
import type { Bookmark, Space } from '@shared/types'
import { currentLocale, t } from './i18n'
import { folderPaths, itemTabIds, walk } from '../core/tree'
import { siteOf } from '../core/urls'
import { QUIT_ACCELERATOR, pauseQuitShortcut, quitNow } from './quit'
import { shortcutAccels } from '@shared/shortcuts'
import { buildMenu, menuIcon } from './menuicons'
import { REPO_URL } from '../features/updates'

const IS_MAC = process.platform === 'darwin'
/** True while a new shortcut is being typed in Settings: no menu shortcut must act. */
let recording = false

/** Suspends (or restores) the menu shortcuts and ⌘Q while a shortcut is being typed in Settings. */
export function setShortcutRecording(b: Browser, on: boolean): void {
  if (on === recording) return
  recording = on
  pauseQuitShortcut(on)
  buildAppMenu(b)
}

/**
 * Copy of a menu template with no shortcuts, while a shortcut is being recorded in Settings. The shortcuts
 * are removed, not just "unregistered": `registerAccelerator: false` only works on Windows and Linux, and
 * on Mac ⌘W would close the window (⌘Q would quit) while the combination is typed.
 */
function withoutAccelerators(items: MenuItemConstructorOptions[]): MenuItemConstructorOptions[] {
  return items.map(({ accelerator: _accelerator, ...item }) => ({
    ...item,
    ...(Array.isArray(item.submenu) && { submenu: withoutAccelerators(item.submenu) })
  }))
}

/** Foreground window and tab that a command must target. */
function target(b: Browser) {
  const win = BrowserWindow.getFocusedWindow()
  // An app window behaves like a Little Signet: a single tab, tab commands bring the main window back.
  // Detached devtools window: commands target the inspected page.
  const little = b.little.tabIdOf(win) ?? b.webApps.tabIdOf(win) ?? b.devtools.tabIdOf(win)
  const priv = b.privates.has(win)
  const tabId = little ?? (priv ? b.privates.activeTabOf(win) : win === b.win ? (b.peek.tabId ?? b.space.activeTabId) : null)
  return { win, little: !!little, priv, tabId }
}

/**
 * Summary of the states that check menu items: the menu is only rebuilt when it changes (see Broadcast.apply).
 * The Develop menu follows developer mode and the active tab (responsive mode size changes continuously during
 * a drag: only its presence matters).
 */
export function menuKey(b: Browser): string {
  const { settings } = b.store.state
  const devOverrides = b.model.activeTab()?.devOverrides
  return [
    b.layout.sidebarOn,
    settings.compactMode,
    b.layout.horizontal,
    b.bookmarks.visible,
    JSON.stringify(settings.shortcuts ?? null),
    settings.devMode === true ? `${b.space.activeTabId}:${JSON.stringify({ ...devOverrides, responsive: !!devOverrides?.responsive })}` : ''
  ].join()
}

export function buildAppMenu(b: Browser): void {
  const cur = () => target(b)
  /** Command that only makes sense in the main window: from a Little Signet or a private window, it brings the main window back first. */
  const inMain = (fn: () => void) => () => {
    const c = cur()
    if (c.little || c.priv) b.window.show()
    fn()
  }
  /** Tab command: in the foreground private window, otherwise in the main window. */
  const tabs = (inPrivate: (win: BrowserWindow | null) => void, fn: () => void) => () => {
    const c = cur()
    if (c.priv) inPrivate(c.win)
    else inMain(fn)()
  }

  /** Find in page: no effect in a Little Signet or without an active tab. */
  const inPage = (fn: () => void) => () => {
    if (cur().win === b.win && b.space.activeTabId) fn()
  }

  const overrides = b.store.state.settings.shortcuts
  /**
   * Customizable command (see src/shared/shortcuts.ts): its current shortcut, plus one hidden entry per additional
   * default shortcut (a menu item has only one shortcut).
   */
  const command = (id: string, item: MenuItemConstructorOptions): MenuItemConstructorOptions[] => {
    const [main, ...more] = shortcutAccels(id, overrides, IS_MAC)
    return [
      { ...item, accelerator: main },
      ...more.map((accelerator) => ({ label: item.label, click: item.click, accelerator, visible: false, acceleratorWorksWhenHidden: true }))
    ]
  }

  const template: MenuItemConstructorOptions[] = [
    {
      label: 'Signet',
      submenu: [
        { label: t('menu.app.about'), role: 'about' },
        { type: 'separator' },
        { label: t('menu.settings'), icon: menuIcon('settings'), accelerator: 'CmdOrCtrl+,', click: () => b.settings.open() },
        { type: 'separator' },
        // Hide / Show All: macOS-specific roles.
        ...(IS_MAC
          ? ([
              { label: t('menu.app.hide'), role: 'hide' },
              { label: t('menu.app.hideOthers'), role: 'hideOthers' },
              { label: t('menu.app.showAll'), role: 'unhide' },
              { type: 'separator' }
            ] satisfies MenuItemConstructorOptions[])
          : []),
        // Accelerator shown but not registered: ⌘Q is handled by quit.ts (hold / double press).
        { label: t('menu.app.quit'), accelerator: QUIT_ACCELERATOR, registerAccelerator: false, click: () => quitNow() }
      ]
    },
    {
      label: t('menu.file.title'),
      submenu: [
        ...command('newTab', {
          label: t('menu.file.newTab'),
          icon: menuIcon('newTab'),
          click: tabs(
            (win) => b.privates.newTab(win),
            () => b.overlay.openPalette('new-tab')
          )
        }),
        ...command('newLittle', { label: t('menu.file.newLittleSignet'), click: () => b.little.open('https://www.google.com') }),
        ...command('newPrivate', { label: t('menu.file.newPrivateWindow'), icon: menuIcon('private'), click: () => b.privates.open() }),
        ...command('newNote', { label: t('menu.file.newNote'), click: () => b.notes.create('note') }),
        ...command('newEasel', { label: t('menu.file.newEasel'), click: () => b.notes.create('easel') }),
        ...command('reopenTab', { label: t('menu.file.reopenTab'), click: inMain(() => b.tabs.reopenClosed()) }),
        { type: 'separator' },
        ...command('closeTab', {
          label: t('menu.file.closeTab'),
          click: () => {
            const { win, little, priv, tabId } = cur()
            if (priv) b.privates.closeActive(win)
            else if (little || !win || win !== b.win) win?.close()
            else if (b.peek.tabId) b.overlay.close()
            else if (tabId) b.tabs.close(tabId)
            else win.close()
          }
        }),
        ...command('closeWindow', { label: t('menu.file.closeWindow'), click: () => BrowserWindow.getFocusedWindow()?.close() }),
        { type: 'separator' },
        ...command('capture', { label: t('menu.file.capture'), icon: menuIcon('capture'), click: () => b.capture.start(cur().tabId) }),
        ...command('savePage', { label: t('menu.file.savePage'), click: () => void b.pageTools.savePage(cur().tabId) }),
        ...command('exportPdf', { label: t('menu.file.exportPdf'), click: () => void b.pageTools.savePdf(cur().tabId) }),
        ...command('print', { label: t('menu.file.print'), icon: menuIcon('print'), click: () => b.pageTools.print(cur().tabId) }),
        // Sites installed as apps: the launcher is a macOS app, so Mac only.
        ...(IS_MAC
          ? [
              { type: 'separator' as const },
              ...command('installApp', {
                label: t('menu.file.installApp'),
                click: () => {
                  const c = cur()
                  void b.webApps.promptInstall(c.win === b.win ? c.tabId : null)
                }
              })
            ]
          : [])
      ]
    },
    {
      label: t('menu.edit.title'),
      submenu: [
        { label: t('menu.edit.undo'), role: 'undo' },
        { label: t('menu.edit.redo'), role: 'redo' },
        { type: 'separator' },
        { label: t('menu.edit.cut'), role: 'cut' },
        { label: t('menu.edit.copy'), role: 'copy' },
        { label: t('menu.edit.paste'), role: 'paste' },
        // No role here: macOS replaces the image of its standard entry, which erases the empty icon and shifts the label.
        {
          label: t('menu.edit.pasteMatchStyle'),
          accelerator: 'Alt+Shift+CmdOrCtrl+V',
          click: () => webContents.getFocusedWebContents()?.pasteAndMatchStyle()
        },
        { label: t('menu.edit.selectAll'), role: 'selectAll' },
        { type: 'separator' },
        ...command('find', { label: t('menu.edit.find'), icon: menuIcon('find'), click: inPage(() => b.find.showOnActive()) }),
        ...command('findNext', { label: t('menu.edit.findNext'), click: inPage(() => b.find.stepOrShow(true)) }),
        ...command('findPrev', { label: t('menu.edit.findPrevious'), click: inPage(() => b.find.stepOrShow(false)) })
      ]
    },
    {
      label: t('menu.view.title'),
      submenu: [
        // Toggles: check on the left when the option is active (menu rebuilt on every change, see Browser.apply).
        ...command('toggleSidebar', {
          label: t('menu.view.toggleSidebar'),
          type: 'checkbox',
          checked: b.layout.sidebarOn,
          enabled: !b.layout.horizontal,
          click: inMain(() => b.shell.toggleSidebar())
        }),
        ...command('compactMode', {
          label: t('menu.view.compact'),
          type: 'checkbox',
          checked: b.store.state.settings.compactMode,
          click: inMain(() => b.shell.toggleCompact())
        }),
        // On Mac, ⌘⇧S as well (hidden entry), see src/shared/shortcuts.ts.
        ...command('tabLayout', {
          label: t('menu.view.tabsTop'),
          type: 'checkbox',
          checked: b.layout.horizontal,
          click: inMain(() => b.shell.toggleTabLayout())
        }),
        ...command('bookmarksBar', {
          label: t('menu.view.showBookmarksBar'),
          type: 'checkbox',
          checked: b.bookmarks.visible,
          click: inMain(() => b.bookmarks.toggleBar())
        }),
        // UI mods: native menu, which no CSS can hide (fallback if a mod makes the UI unusable).
        { label: t('menu.view.mods'), type: 'checkbox', checked: !b.mods.allDisabled, click: () => b.mods.setDisabled(!b.mods.allDisabled) },
        { type: 'separator' },
        ...command('reload', { label: t('menu.view.reload'), icon: menuIcon('reload'), click: () => b.tabs.reload(cur().tabId) }),
        ...command('forceReload', { label: t('menu.view.hardReload'), click: () => b.tabs.reload(cur().tabId, true) }),
        { type: 'separator' },
        ...command('zoomIn', { label: t('menu.view.zoomIn'), click: () => b.tabs.zoom(cur().tabId, 0.5) }),
        ...command('zoomOut', { label: t('menu.view.zoomOut'), click: () => b.tabs.zoom(cur().tabId, -0.5) }),
        ...command('actualSize', { label: t('menu.view.actualSize'), click: () => b.tabs.zoom(cur().tabId, 0) }),
        { type: 'separator' },
        ...command('customizeSite', { label: t('menu.view.customizeSite'), click: () => b.boosts.openForTab(cur().tabId) }),
        ...command('reader', {
          label: t('menu.view.reader'),
          icon: menuIcon('reader'),
          click: () => {
            const { tabId } = cur()
            if (tabId && b.reader.canRead(tabId)) b.reader.toggle(tabId)
          }
        }),
        ...command('translate', {
          label: t('menu.view.translate'),
          icon: menuIcon('translate'),
          click: () => {
            const { tabId } = cur()
            if (tabId && b.translator.canTranslate(tabId)) b.translator.toggle(tabId)
          }
        }),
        { type: 'separator' },
        { label: t('menu.view.fullScreen'), role: 'togglefullscreen' },
        ...command('devTools', { label: t('menu.view.devTools'), icon: menuIcon('devTools'), click: () => b.devtools.toggle(cur().tabId) }),
        ...command('jsConsole', { label: t('menu.view.jsConsole'), click: () => b.devtools.showConsole(cur().tabId) }),
        ...command('viewSource', { label: t('menu.view.viewSource'), click: () => b.pageTools.viewSource(cur().tabId) }),
        { type: 'separator' },
        // Fallback if the window UI shows up empty (native menu: always accessible).
        { label: t('menu.view.reloadInterface'), click: () => b.window.reloadInterface() }
      ]
    },
    {
      label: t('menu.navigate.title'),
      submenu: [
        ...command('back', { label: t('menu.navigate.back'), click: () => b.tabs.goBack(cur().tabId) }),
        ...command('forward', { label: t('menu.navigate.forward'), click: () => b.tabs.goForward(cur().tabId) }),
        { type: 'separator' },
        ...command('openAddress', {
          label: t('menu.navigate.openAddress'),
          click: () => {
            const c = cur()
            if (c.priv) b.privates.focusAddress(c.win)
            else if (!c.little) b.overlay.editAddress()
          }
        }),
        ...command('library', { label: t('menu.navigate.library'), icon: menuIcon('library'), click: () => b.library.open() }),
        ...command('copyUrl', { label: t('menu.navigate.copyUrl'), click: () => b.tabs.copyUrl(cur().tabId) }),
        ...command('bookmark', { label: t('menu.navigate.toggleBookmark'), icon: menuIcon('bookmark'), click: inMain(() => b.bookmarks.toggle(null)) }),
        ...command('pinTab', { label: t('menu.navigate.togglePin'), click: inMain(() => b.space.activeTabId && b.arrange.togglePin(b.space.activeTabId)) }),
        ...command('openSplit', { label: t('menu.navigate.splitView'), icon: menuIcon('split'), click: inMain(() => b.overlay.openPalette('split')) }),
        { type: 'separator' },
        // ⇧⌘] and ⇧⌘[ as well (hidden entries), see src/shared/shortcuts.ts.
        ...command('nextTab', {
          label: t('menu.navigate.nextTab'),
          click: tabs(
            (win) => b.privates.cycle(win, 1),
            () => b.tabs.cycle(1)
          )
        }),
        ...command('prevTab', {
          label: t('menu.navigate.previousTab'),
          click: tabs(
            (win) => b.privates.cycle(win, -1),
            () => b.tabs.cycle(-1)
          )
        }),
        ...command('nextSpace', { label: t('menu.navigate.nextSpace'), click: inMain(() => b.spaces.cycle(1)) }),
        ...command('prevSpace', { label: t('menu.navigate.previousSpace'), click: inMain(() => b.spaces.cycle(-1)) }),
        { type: 'separator' },
        ...Array.from({ length: 9 }, (_, i): MenuItemConstructorOptions => ({
          label: i === 8 ? t('menu.navigate.lastTab') : t('menu.navigate.tab', { number: i + 1 }),
          accelerator: `CmdOrCtrl+${i + 1}`,
          click: tabs(
            (win) => b.privates.goto(win, i + 1),
            () => b.tabs.gotoNumber(i + 1)
          )
        })),
        { type: 'separator' },
        ...Array.from({ length: 9 }, (_, i): MenuItemConstructorOptions => ({
          label: t('menu.navigate.space', { number: i + 1 }),
          // ⌃1…9 on Mac; elsewhere Ctrl+1…9 goes to tabs, hence Alt+1…9.
          accelerator: process.platform === 'darwin' ? `Ctrl+${i + 1}` : `Alt+${i + 1}`,
          click: inMain(() => b.spaces.goto(i + 1))
        }))
      ]
    },
    // Develop menu (like Safari): only with developer mode. The checks follow the active tab.
    ...(b.store.state.settings.devMode === true
      ? [
          {
            label: t('menu.develop.title'),
            submenu: [
              { label: t('menu.view.devTools'), icon: menuIcon('devTools'), click: () => b.devtools.toggle(cur().tabId) },
              { label: t('menu.view.jsConsole'), click: () => b.devtools.showConsole(cur().tabId) },
              ...command('responsive', {
                label: t('develop.responsive'),
                type: 'checkbox',
                checked: (() => {
                  const id = cur().tabId
                  return !!id && !!b.develop.get(id).responsive
                })(),
                click: () => b.develop.toggleResponsive(cur().tabId)
              }),
              { type: 'separator' },
              ...b.develop.menuItems(() => cur().tabId, b.space.activeTabId, true)
            ] satisfies MenuItemConstructorOptions[]
          }
        ]
      : []),
    {
      label: t('menu.window.title'),
      role: 'window',
      submenu: [
        { label: t('menu.window.minimize'), role: 'minimize' },
        // Zoom / Bring All to Front: macOS-specific roles.
        ...(IS_MAC
          ? ([
              { label: t('menu.window.zoom'), role: 'zoom' },
              { type: 'separator' },
              { label: t('menu.window.bringAllToFront'), role: 'front' }
            ] satisfies MenuItemConstructorOptions[])
          : [])
      ]
    },
    {
      label: t('menu.help.title'),
      // macOS adds its menu search field to the menu with this role.
      role: 'help',
      submenu: [
        { label: t('menu.help.checkUpdates'), click: () => void b.updates.check(true) },
        { type: 'separator' },
        { label: t('menu.help.releaseNotes'), click: () => b.updates.openPage(`${REPO_URL}/releases`) },
        { label: t('menu.help.reportIssue'), click: () => b.updates.openPage(`${REPO_URL}/issues/new`) },
        { label: t('menu.help.github'), click: () => b.updates.openPage(REPO_URL) }
      ]
    }
  ]
  Menu.setApplicationMenu(buildMenu(recording ? withoutAccelerators(template) : template))
  app.setAboutPanelOptions({ applicationName: 'Signet' })
}

export function showTabMenu(b: Browser, tabId: string): void {
  const { state } = b.store
  const tab = state.tabs[tabId]
  const loc = b.model.locate(tabId)
  if (!tab || !loc) return
  const active = b.space.activeTabId
  const items: MenuItemConstructorOptions[] = [
    { label: loc.zone === 'today' ? t('menu.tab.pin') : t('menu.tab.unpin'), icon: menuIcon('pin'), click: () => b.arrange.togglePin(tabId) },
    {
      label: loc.zone === 'favorites' ? t('menu.tab.removeFavorite') : t('menu.tab.addFavorite'),
      icon: menuIcon('favorite'),
      click: () => b.arrange.toggleFavorite(tabId)
    }
  ]
  if (tabId !== active && loc.zone !== 'favorites' && b.model.splits.canSplit()) {
    items.push({ label: t('menu.tab.splitWithActive'), icon: menuIcon('split'), click: () => b.splitView.splitActiveWith(tabId) })
  }
  if (loc.item.kind === 'split') {
    const split = loc.item
    const current = split.layout ?? 'columns'
    const layoutItem = (label: string, layout: SplitLayout, enabled = true): MenuItemConstructorOptions => ({
      label,
      type: 'radio',
      checked: current === layout,
      enabled,
      click: () => b.splitView.setLayout(split.id, layout)
    })
    items.push(
      { label: t('menu.tab.removeFromSplit'), click: () => b.splitView.removeFromSplit(tabId) },
      {
        label: t('menu.tab.layout'),
        submenu: [
          layoutItem(t('menu.tab.layoutColumns'), 'columns'),
          layoutItem(t('menu.tab.layoutRows'), 'rows'),
          layoutItem(t('menu.tab.layoutGrid'), 'grid', split.tabIds.length >= 3),
          { type: 'separator' },
          { label: t('menu.tab.equalize'), click: () => b.splitView.setSizes(split.id, []) }
        ]
      }
    )
  }
  if (loc.zone === 'today' && loc.item.kind !== 'folder' && loc.item.group) {
    items.push({ label: t('menu.tab.leaveGroup'), click: () => b.groups.leave(tabId) })
  }
  items.push(
    { label: b.bookmarks.findUrl(tab.url) ? t('menu.tab.removeBookmark') : t('menu.tab.addBookmark'), click: () => b.bookmarks.toggle(tabId) },
    { label: t('menu.tab.duplicate'), click: () => b.tabs.duplicate(tabId) },
    { label: t('menu.tab.copyUrl'), icon: menuIcon('link'), click: () => b.tabs.copyUrl(tabId) },
    ...b.link.menuItems(tabId)
  )
  if (IS_MAC && /^https?:\/\//i.test(tab.url)) {
    const installed = b.webApps.appFor(tab.url)
    items.push(
      installed
        ? { label: t('menu.tab.openInApp', { name: installed.name }), click: () => b.webApps.moveToApp(tabId, installed.id) }
        : { label: t('menu.tab.installApp'), click: () => void b.webApps.promptInstall(tabId) }
    )
  }
  if (loc.list.length > 1) {
    items.push(
      { label: t('menu.tab.moveUp'), enabled: loc.index > 0, click: () => b.arrange.moveTabStep(tabId, -1) },
      { label: t('menu.tab.moveDown'), enabled: loc.index < loc.list.length - 1, click: () => b.arrange.moveTabStep(tabId, 1) }
    )
  }
  const others = state.spaces.filter((s) => s.id !== (loc.space?.id ?? state.activeSpaceId))
  if (others.length) {
    items.push({
      label: t('menu.tab.moveToSpace'),
      submenu: others.map((s) => ({ label: `${s.icon} ${s.name}`, click: () => b.arrange.moveTabToSpace(tabId, s.id) }))
    })
  }
  const site = /^https?:\/\//i.test(tab.url) ? siteOf(tab.url) : ''
  if (site) {
    const rule = state.settings.routes.find((r) => r.pattern === site)
    items.push({
      label: t('menu.tab.alwaysOpenIn', { site }),
      submenu: [
        ...state.spaces.map((s): MenuItemConstructorOptions => ({
          label: `${s.icon} ${s.name}`,
          type: 'checkbox',
          checked: rule?.spaceId === s.id,
          click: () => b.spaces.addRoute(site, s.id)
        })),
        ...(rule ? [{ type: 'separator' as const }, { label: t('menu.tab.stopRouting'), click: () => b.spaces.removeRoute(rule.id) }] : [])
      ]
    })
  }
  if (loc.zone !== 'favorites' && loc.space) {
    // A live folder fills itself: no tab is put in it.
    const folders = folderPaths(loc.space.pinned).filter(({ folder }) => !folder.live)
    items.push({
      label: t('menu.tab.addToFolder'),
      submenu: [
        ...folders.map(({ folder, label }) => ({ label, click: () => b.folders.addTo(tabId, folder.id) })),
        ...(folders.length ? [{ type: 'separator' as const }] : []),
        { label: t('menu.tab.newFolder'), click: () => b.folders.addTo(tabId, null) }
      ]
    })
  }
  items.push(
    { type: 'separator' },
    { label: t('menu.tab.rename'), click: () => b.floating.send('tab:rename-request', { tabId }) },
    { label: t('menu.tab.chooseIcon'), click: () => b.floating.send('icon:request', { kind: 'tab', tabId }) }
  )
  if (tab.customTitle || tab.customIcon) {
    items.push({
      label: t('menu.tab.resetNameAndIcon'),
      click: () => {
        renameTab(b, tabId, null)
        setTabIcon(b, tabId, null)
      }
    })
  }
  items.push(
    { type: 'separator' },
    { label: tab.pinnedUrl ? t('menu.tab.unload') : t('menu.tab.close'), icon: menuIcon('close'), click: () => b.tabs.close(tabId) }
  )
  buildMenu(items).popup({ window: b.win ?? undefined })
}

/** Hues offered for folders: name key and HSL angle. */
const FOLDER_HUES: [Parameters<typeof t>[0], number][] = [
  ['menu.folder.hue.red', 0],
  ['menu.folder.hue.orange', 28],
  ['menu.folder.hue.yellow', 48],
  ['menu.folder.hue.green', 140],
  ['menu.folder.hue.turquoise', 180],
  ['menu.folder.hue.blue', 215],
  ['menu.folder.hue.purple', 265],
  ['menu.folder.hue.pink', 330]
]

/** Detail of the space deletion confirmation: what disappears, with the counts. */
function spaceDeleteDetail(space: Space): string {
  let folders = 0
  walk(space.pinned, ({ item }) => {
    if (item.kind === 'folder') folders++
  })
  const countMarks = (list: Bookmark[]): number => list.reduce((n, b) => n + (b.kind === 'link' ? 1 : countMarks(b.items)), 0)
  const tabs = [...space.pinned, ...space.today].flatMap(itemTabIds).length
  const marks = countMarks(space.bookmarks ?? [])
  const parts = [
    tabs > 0 ? t('dialog.deleteSpace.tabs', { count: tabs }) : '',
    folders > 0 ? t('dialog.deleteSpace.folders', { count: folders }) : '',
    marks > 0 ? t('dialog.deleteSpace.bookmarks', { count: marks }) : ''
  ].filter(Boolean)
  const base = parts.length
    ? t('dialog.deleteSpace.detail', { summary: new Intl.ListFormat(currentLocale(), { style: 'long', type: 'conjunction' }).format(parts) })
    : t('dialog.deleteSpace.detailEmpty')
  return space.profile === 'isolated' ? `${base} ${t('dialog.deleteSpace.isolatedNote')}` : base
}

export function showFolderMenu(b: Browser, folderId: string): void {
  const folder = b.model.findFolder(folderId)?.item
  if (!folder) return
  const confirmDelete = async () => {
    const options = {
      type: 'warning' as const,
      message: t('dialog.deleteFolder.message', { name: folder.name }),
      detail: t('dialog.deleteFolder.detail'),
      buttons: [t('dialog.deleteFolder.confirm'), t('dialog.cancel')],
      defaultId: 1,
      cancelId: 1
    }
    const { response } = b.win ? await dialog.showMessageBox(b.win, options) : await dialog.showMessageBox(options)
    if (response === 0) b.folders.delete(folderId, true)
  }
  // Live folder: update and source first; no subfolder (its content is managed).
  const live: MenuItemConstructorOptions[] = folder.live
    ? [
        { label: t('menu.folder.refresh'), icon: menuIcon('reload'), enabled: !folder.live.refreshing, click: () => void b.liveFolders.refresh(folderId) },
        { label: t('menu.folder.editSource'), click: () => b.liveFolders.openForm(folderId) },
        { type: 'separator' }
      ]
    : []
  const items: MenuItemConstructorOptions[] = [
    ...live,
    { label: t('menu.folder.rename'), click: () => b.folders.requestRename(folderId) },
    { label: t('menu.tab.chooseIcon'), click: () => b.floating.send('icon:request', { kind: 'folder', folderId }) },
    ...(folder.live ? [] : [{ label: t('menu.folder.newInside'), icon: menuIcon('newFolder'), click: () => b.folders.create(folderId) }]),
    {
      label: t('menu.folder.color'),
      submenu: [
        { label: t('menu.folder.colorNone'), type: 'checkbox', checked: folder.hue === undefined, click: () => b.folders.setHue(folderId, null) },
        { type: 'separator' },
        ...FOLDER_HUES.map(([key, hue]): MenuItemConstructorOptions => ({
          label: t(key),
          type: 'checkbox',
          checked: folder.hue === hue,
          click: () => b.folders.setHue(folderId, hue)
        }))
      ]
    },
    { type: 'separator' },
    // The folder becomes a group of today's tabs again (subfolders flattened).
    { label: t('menu.folder.unpin'), enabled: itemTabIds(folder).length > 0, click: () => b.arrange.unpinFolder(folderId) },
    { label: t('menu.folder.delete'), icon: menuIcon('delete'), click: () => b.folders.delete(folderId, false) },
    { label: t('menu.folder.deleteWithTabs'), click: () => void confirmDelete() }
  ]
  buildMenu(items).popup({ window: b.win ?? undefined })
}

/** Menu of the sidebar's "New folder" button: simple folder or live folder. */
export function showAddFolderMenu(b: Browser): void {
  const items: MenuItemConstructorOptions[] = [
    { label: t('menu.folder.newFolder'), icon: menuIcon('newFolder'), click: () => b.folders.create() },
    { label: t('menu.folder.newLiveFolder'), icon: menuIcon('liveFolder'), click: () => b.liveFolders.openForm(null) }
  ]
  buildMenu(items).popup({ window: b.win ?? undefined })
}

/** Menu of a today's-tabs group header. */
export function showGroupMenu(b: Browser, groupId: string): void {
  const group = b.model.groups.header(groupId)
  if (!group) return
  const items: MenuItemConstructorOptions[] = [
    { label: t('menu.group.rename'), click: () => b.folders.requestRename(groupId) },
    { label: t('menu.tab.chooseIcon'), click: () => b.floating.send('icon:request', { kind: 'group', groupId }) },
    {
      label: t('menu.folder.color'),
      submenu: FOLDER_HUES.map(([key, hue]): MenuItemConstructorOptions => ({
        label: t(key),
        type: 'checkbox',
        checked: group.hue === hue,
        click: () => b.groups.setHue(groupId, hue)
      }))
    },
    { label: group.open ? t('menu.group.collapse') : t('menu.group.expand'), click: () => b.groups.toggle(groupId) },
    { type: 'separator' },
    { label: t('menu.group.pin'), icon: menuIcon('pin'), click: () => b.arrange.pinGroup(groupId) },
    { label: t('menu.group.ungroup'), click: () => b.groups.ungroup(groupId) },
    { label: t('menu.group.close'), icon: menuIcon('close'), click: () => b.groups.closeById(groupId) }
  ]
  buildMenu(items).popup({ window: b.win ?? undefined })
}

export function showSpaceMenu(b: Browser, spaceId: string): void {
  const { spaces } = b.store.state
  const space = spaces.find((s) => s.id === spaceId)
  if (!space) return
  const items: MenuItemConstructorOptions[] = [
    { label: t('menu.space.edit'), icon: menuIcon('edit'), click: () => b.spaces.edit(spaceId) },
    { type: 'separator' },
    { label: t('menu.space.moveLeft'), enabled: spaces.findIndex((s) => s.id === spaceId) > 0, click: () => b.spaces.move(spaceId, -1) },
    {
      label: t('menu.space.moveRight'),
      enabled: spaces.findIndex((s) => s.id === spaceId) < spaces.length - 1,
      click: () => b.spaces.move(spaceId, 1)
    },
    { type: 'separator' },
    {
      label: t('menu.space.isolatedProfile'),
      type: 'checkbox',
      checked: space.profile === 'isolated',
      click: () => void b.spaces.setProfile(spaceId, space.profile === 'isolated' ? 'shared' : 'isolated')
    },
    { label: t('menu.space.clearData'), enabled: space.profile === 'isolated', click: () => void b.spaces.clearData(spaceId) },
    { type: 'separator' },
    { label: t('menu.space.newLiveFolder'), enabled: spaceId === b.space.id, click: () => b.liveFolders.openForm(null) },
    { type: 'separator' },
    {
      label: t('menu.space.delete'),
      icon: menuIcon('delete'),
      enabled: spaces.length > 1,
      click: () =>
        void (async () => {
          const options = {
            type: 'warning' as const,
            message: t('dialog.deleteSpace.message', { name: space.name }),
            detail: spaceDeleteDetail(space),
            buttons: [t('dialog.deleteSpace.confirm'), t('dialog.cancel')],
            defaultId: 1,
            cancelId: 1
          }
          const { response } = b.win ? await dialog.showMessageBox(b.win, options) : await dialog.showMessageBox(options)
          if (response === 0) b.spaces.delete(spaceId)
        })()
    }
  ]
  buildMenu(items).popup({ window: b.win ?? undefined })
}

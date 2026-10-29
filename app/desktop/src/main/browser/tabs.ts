import { clipboard, nativeTheme, WebContentsView, type MenuItem, type MenuItemConstructorOptions, type Session } from 'electron'
import { IS_MAC } from '../app/platform'
import type { Browser } from './browser'
import { engineSearchUrl, siteOf } from '../core/urls'
import { emulateLanguages } from '../web/languages'
import { displayUrl, guardNavigation, isEngineErrorPage, isInternalUrl, resolveInternal } from '../web/internal'
import { crashDocument, ERROR_CSS, injectionScript, insecureSiteScript, insecureSiteText, loadErrorText } from '../web/errorpage'
import { t } from '../app/i18n'
import { buildMenu, menuIcon } from '../app/menuicons'
import { tabContents } from '../web/guard'
import { fontPreferences } from '@shared/pagesettings'
import { CONTINUE_TITLE } from './https-only'

/**
 * Addresses of `<link rel="icon">` reserved for a theme or medium (`media` attribute, e.g. `(prefers-color-scheme: dark)`)
 * that match now, in document order. Chromium announces all icons regardless of `media`:
 * without this sort, a site with one icon for the light theme and one for the dark would always keep the first.
 * Icons without `media` are fallback icons (e.g. Densho puts one before the other two): they only come after.
 */
const MATCHING_ICONS = `(() => {
  const links = [...document.querySelectorAll('link[rel~="icon" i][href]')]
  return {
    matching: links.filter((l) => l.media.trim() && matchMedia(l.media).matches).map((l) => l.href),
    plain: links.filter((l) => !l.media.trim()).map((l) => l.href)
  }
})()`

/** Redoes the favicon choice of each live tab (the theme changed). */
const faviconRefreshers = new Set<() => void>()
let watchingTheme = false

function watchThemeForFavicons(): void {
  if (watchingTheme) return
  watchingTheme = true
  // Short delay: pages must first receive the new theme so that `matchMedia` answers correctly.
  nativeTheme.on('updated', () => setTimeout(() => faviconRefreshers.forEach((refresh) => refresh()), 150))
}

/** Views whose displayed page is the crash page (`data:` address): reloading must reopen the original address. */
const crashed = new WeakMap<object, string>()

/** Reloads a view; after a crash, reloads the original address rather than the crash page. */
/** Last form submission per tab: the navigation that follows is not a link, so no Peek. */
const lastSubmit = new WeakMap<Electron.WebContents, number>()
export function noteFormSubmit(wc: Electron.WebContents): void {
  lastSubmit.set(wc, Date.now())
}

export function reloadContents(wc: Electron.WebContents, force = false): void {
  const url = crashed.get(wc)
  if (url) wc.loadURL(url).catch(() => {})
  else if (force) wc.reloadIgnoringCache()
  else wc.reload()
}

/** Creates a tab's web view and relays its events to its `TabInfo`. */
export function createTabView(b: Browser, tabId: string, ses?: Session): WebContentsView {
  const view = new WebContentsView({
    // Font sizes (Settings › Appearance) can only be given at creation: they reach the pages of views created from now on.
    webPreferences: { sandbox: true, contextIsolation: true, nodeIntegration: false, ...fontPreferences(b.store.state.settings), ...(ses && { session: ses }) }
  })
  view.setBackgroundColor('#ffffff')
  view.setBorderRadius(12)
  const wc = view.webContents
  tabContents.add(wc)
  // Each module (Boost, media, site color, developer mode, extensions…) follows the tab with its own
  // listeners, notably `destroyed`: the default limit (10) is exceeded without a leak. 25 would still signal a real leak.
  wc.setMaxListeners(25)
  emulateLanguages(wc, b.store.state.settings.languages)
  b.boosts.attach(wc)
  b.media.attach(tabId, wc)
  b.siteColor.attach(tabId, wc)
  b.devMode.attach(tabId, wc)
  b.fullscreen.attach(tabId, wc)
  const info = () => b.store.state.tabs[tabId]
  // Private window tab: nothing in the history, its links stay in its window.
  const priv = b.privates.owns(tabId)
  // Installed app tab (its window is created after the view: we ask every time).
  const inApp = () => b.webApps.owns(tabId)
  /**
   * Opens a link in a new tab: in its group, in its private window if applicable,
   * or in the main window from an app.
   */
  const openNext = (url: string, activate = true) =>
    priv ? b.privates.openFrom(tabId, url, activate) : inApp() ? b.webApps.openInBrowser(url) : b.tabs.open(url, { activate, openerId: tabId })
  // Last main-frame load failure (`token`: the HTTPS-only warning is shown instead of the error page) and original address of a crashed page.
  let failed: { url: string; code: number; token?: string } | null = null
  let crashUrl = ''

  wc.on('page-title-updated', (e, rawTitle) => {
    // "Continue to site" on the HTTPS-only warning: the page asks through its title (see `insecureSiteScript`).
    if (rawTitle.startsWith(CONTINUE_TITLE)) {
      e.preventDefault()
      const http = b.httpsOnly.continueFrom(tabId, wc.session, rawTitle)
      if (http) b.tabs.loadUrl(tabId, http)
      return
    }
    let title = rawTitle
    if (!info()) return
    title = displayUrl(title) // some engine page titles are their real address
    b.broadcast.patchTab(tabId, { title })
    if (!priv) b.history.update(displayUrl(wc.getURL()), { title })
    if (b.peek.tabId === tabId) b.peek.refresh()
  })
  // Favicon: among the announced icons, the first whose `media` matches the current theme (see MATCHING_ICONS).
  let announced: string[] = []
  const applyFavicon = async () => {
    const list = announced
    if (!list[0] || wc.isDestroyed()) return
    let found = {} as { matching?: unknown; plain?: unknown }
    try {
      // Via the main frame: immediate, even during loading (the webContents would wait for it to finish).
      found = ((await wc.mainFrame.executeJavaScript(MATCHING_ICONS)) as typeof found) ?? {}
    } catch {
      // page being replaced: the first announced icon
    }
    // First an icon of the current theme, otherwise an icon without `media`, otherwise (page already replaced) the first announced.
    const pick = (v: unknown) => (Array.isArray(v) ? v.find((u) => typeof u === 'string' && list.includes(u)) : undefined)
    const url = pick(found.matching) ?? pick(found.plain) ?? list[0]
    if (announced !== list || wc.isDestroyed() || !info() || info().favicon === url) return
    b.broadcast.patchTab(tabId, { favicon: url })
    if (!priv) b.history.update(displayUrl(wc.getURL()), { favicon: url })
  }
  const refreshFavicon = () => void applyFavicon()
  faviconRefreshers.add(refreshFavicon)
  watchThemeForFavicons()
  wc.on('page-favicon-updated', (_e, favicons) => {
    if (!info() || !favicons[0]) return
    announced = favicons
    void applyFavicon()
  })
  wc.on('did-start-loading', () => info() && b.broadcast.patchTab(tabId, { loading: true }))
  wc.on('did-stop-loading', () => info() && b.broadcast.patchTab(tabId, { loading: false, ...navState() }))
  // Load failure: the engine's error page is replaced by a clean page (see `did-finish-load`).
  wc.on('did-fail-load', (_e, code, _desc, url, isMainFrame) => {
    if (!isMainFrame || code === -3) return // -3: navigation canceled by the page or the user
    // HTTPS-only: the upgraded load has no working secure connection, a warning replaces the error page (and the silent http fallback).
    const insecure = info() ? b.httpsOnly.insecureFailure(tabId, url, code) : null
    if (insecure) {
      b.tabs.clearTypedUpgrade(tabId)
      failed = { url, code, token: b.httpsOnly.warn(tabId, insecure) }
      b.broadcast.patchTab(tabId, { loading: false })
      return
    }
    if (info() && b.tabs.retryTypedHttp(tabId, url, code)) return // input without scheme: http fallback, no error page
    failed = { url, code }
    if (info()) b.broadcast.patchTab(tabId, { loading: false })
  })
  wc.on('render-process-gone', (_e, details) => {
    if (details.reason === 'clean-exit' || details.reason === 'killed' || wc.isDestroyed() || !info()) return
    crashUrl = info().url
    b.broadcast.patchTab(tabId, { loading: false })
    if (!/^(https?|file):/i.test(crashUrl)) return
    crashed.set(wc, crashUrl)
    wc.loadURL(`data:text/html;charset=utf-8,${encodeURIComponent(crashDocument(crashUrl))}`).catch(() => {})
  })

  const navState = () => ({
    canGoBack: wc.navigationHistory.canGoBack(),
    canGoForward: wc.navigationHistory.canGoForward()
  })
  const visited = (realUrl: string, committed = false) => {
    if (!info()) return
    // Error or crash page: the tab keeps the original address and the history stays intact.
    const fallback =
      crashed.has(wc) && realUrl.startsWith('data:') ? crashUrl : isEngineErrorPage(realUrl) || isEngineErrorPage(wc.getURL()) ? failed?.url : undefined
    if (fallback) {
      b.broadcast.patchTab(tabId, { url: fallback, ...navState() })
      return
    }
    const url = displayUrl(realUrl) // the UI never sees the engine's real address
    b.broadcast.patchTab(tabId, { url, ...navState() })
    b.passwords.onNavigate(tabId, url)
    // A page committed after a navigation the command bar started counts as typed (private windows record nothing).
    const typed = committed && b.tabs.takeTyped(tabId)
    if (!priv && !b.devMode.keepOutOfHistory(url)) b.history.record(url, wc.getTitle(), info().favicon, typed)
    if (b.peek.tabId === tabId) b.peek.refresh()
  }
  // The blocker's counter restarts from zero on each main-frame navigation.
  const wcId = wc.id
  wc.once('destroyed', () => {
    b.adblock.resetBlocked(wcId)
    b.reader.forget(tabId)
    b.tabs.forgetTyped(tabId)
    faviconRefreshers.delete(refreshFavicon)
    b.httpsOnly.forget(tabId)
  })
  wc.on('did-start-navigation', (details) => {
    if (details.isMainFrame && !details.isSameDocument) {
      b.adblock.resetBlocked(wc.id)
      b.tabs.typedStart(tabId, details.url)
      if (!details.url.startsWith('data:')) crashed.delete(wc)
      // HTTPS-only safety net for what bypasses `will-navigate` (back/forward to an http entry, restored history).
      if (/^http:/i.test(details.url)) {
        const https = b.httpsOnly.resolve(tabId, wc.session, details.url)
        if (https !== details.url) wc.loadURL(https).catch(() => {})
      }
    }
  })
  wc.on('did-navigate', (_e, url) => {
    // Page validated: the previous page's reader, translation and icons disappear with it.
    announced = []
    b.tabs.clearTypedUpgrade(tabId)
    b.httpsOnly.onCommitted(tabId, url)
    b.pageZoom.apply(wc)
    b.reader.onNavigate(tabId)
    b.translator.onNavigate(tabId)
    b.find.onNavigate(tabId)
    visited(url, true)
    b.views.queueHistory(tabId)
  })
  wc.on('did-navigate-in-page', (_e, url, isMainFrame) => {
    if (!isMainFrame) return
    visited(url)
    b.views.queueHistory(tabId)
    b.reader.onNavigateInPage(tabId, wc)
  })
  wc.on('did-finish-load', () => {
    b.views.queueHistory(tabId)
    // `getURL()` returns the original address, not `chrome-error:`: the script itself checks that it is in the error page.
    if (failed?.token)
      wc.executeJavaScript(insecureSiteScript(insecureSiteText(failed.url), ERROR_CSS, failed.token, CONTINUE_TITLE, wc.navigationHistory.canGoBack())).catch(
        () => {}
      )
    else if (failed) wc.executeJavaScript(injectionScript(loadErrorText(failed.url, failed.code), ERROR_CSS)).catch(() => {})
    // Readable article? Foreign language? (Reader and Translate toolbar buttons)
    // One small check of the content type decides which probe to inject (Readability or JSON viewer).
    else {
      wc.executeJavaScript('document.contentType')
        .then((type: unknown) => {
          const ct = typeof type === 'string' ? type.toLowerCase() : ''
          if (/^(text\/html|application\/xhtml\+xml)$/.test(ct)) void b.reader.probe(tabId, wc)
          else if (/^(application\/([\w.+-]*\+)?json|text\/json)$/.test(ct)) void b.jsonView.probe(wc)
        })
        .catch(() => {})
    }
    if (!wc.getURL().startsWith('data:')) failed = null
  })
  wc.on('audio-state-changed', (e) => info() && b.broadcast.patchTab(tabId, { audible: e.audible }))

  wc.setWindowOpenHandler(({ url, disposition, features }) => {
    // Internal address: refused from a web page; from an internal page, shortcut or new tab.
    if (isInternalUrl(url)) {
      guardNavigation(b, wc.getURL(), url, (real) => openNext(displayUrl(real)))
      return { action: 'deny' }
    }
    if (disposition === 'new-window' && features) {
      // Popup (often an OAuth login): real window, which must be able to answer its opener.
      return {
        action: 'allow',
        overrideBrowserWindowOptions: { width: 520, height: 720, autoHideMenuBar: true }
      }
    }
    // Only web pages reopen here: data:, chrome-extension:… remain subject to Chromium's blocking.
    if (!/^https?:/i.test(url)) return { action: 'deny' }
    if (priv) {
      openNext(url, disposition !== 'background-tab')
    } else if (inApp()) {
      // App: another window of the app on its site, a Signet tab elsewhere.
      b.webApps.openLink(tabId, url)
    } else if (b.store.detached.has(tabId)) {
      // Peek and Little Signet have no tabs around them: we navigate in place.
      wc.loadURL(b.httpsOnly.resolve(tabId, wc.session, resolveInternal(url))).catch(() => {})
    } else if (disposition === 'new-window') {
      b.peek.open(url, tabId, lastPoint)
    } else {
      b.tabs.open(url, { activate: disposition !== 'background-tab', openerId: tabId })
    }
    return { action: 'deny' }
  })

  // A web page cannot go to an internal address; an internal page can (shortcuts included).
  const guard = (details: { url: string; preventDefault: () => void; isMainFrame: boolean }, redirect = false) => {
    if (!isInternalUrl(details.url)) return false
    const same = details.url === wc.getURL()
    if (same) return false
    details.preventDefault()
    // Server redirect: the source is the web, never the previous page (an internal page does not allow it).
    const from = redirect ? 'https://redirect.invalid/' : wc.getURL()
    if (details.isMainFrame) guardNavigation(b, from, details.url, (real) => void wc.loadURL(real).catch(() => {}))
    return true
  }
  wc.on('will-frame-navigate', (details) => {
    if (!details.isMainFrame) guard(details)
  })
  // HTTPS-only mode: a main-frame navigation or redirect to http:// goes to https:// instead (the load failure shows the warning page).
  const upgradeToHttps = (details: { url: string; preventDefault: () => void }) => {
    const https = b.httpsOnly.resolve(tabId, wc.session, details.url)
    if (https === details.url) return
    details.preventDefault()
    wc.loadURL(https).catch(() => {})
  }
  wc.on('will-redirect', (details) => {
    if (details.isMainFrame && !guard(details, true)) upgradeToHttps(details)
  })

  // Last click in the page: without a recent click, the navigation comes from a script (SSO, redirect) and is not intercepted.
  let lastClick = 0
  // Position of the last click (page coordinates): the Peek springs from it.
  let lastPoint: { x: number; y: number } | undefined
  wc.on('input-event', (_e, input) => {
    if (input.type !== 'mouseDown') return
    lastClick = Date.now()
    const { x, y } = input as { x?: number; y?: number }
    lastPoint = typeof x === 'number' && typeof y === 'number' ? { x, y } : undefined
  })
  const hadGesture = () => Date.now() - lastClick < 1500
  const submitted = () => Date.now() - (lastSubmit.get(wc) ?? 0) < 2000

  wc.on('will-navigate', (details) => {
    if (guard(details)) return
    if (!details.isMainFrame) return
    const tab = info()
    const peeks =
      b.store.state.settings.peekExternalLinks &&
      !!tab?.pinnedUrl &&
      siteOf(details.url) !== siteOf(tab.url) &&
      // Only web pages, and navigations started by a gesture (link): not a script or a form.
      /^https?:/i.test(details.url) &&
      hadGesture() &&
      !submitted()
    if (peeks) {
      details.preventDefault()
      b.peek.open(details.url, tabId, lastPoint)
    } else upgradeToHttps(details) // the Peek, if any, upgrades when it loads
  })

  wc.on('before-input-event', (event, input) => {
    if (input.type === 'keyDown' && input.key === 'Escape' && b.peek.tabId === tabId) {
      event.preventDefault()
      b.overlay.close()
    }
  })

  wc.on(
    'context-menu',
    (_e, params) =>
      void (async () => {
        // Mini player entry: only for a normal web page that contains a video.
        // Right-click on a video: certain. Otherwise we query the page, but never delay the menu by more than 100 ms.
        let video = false
        if (!b.store.detached.has(tabId) && /^(https?|file|data):/i.test(wc.getURL())) {
          if (params.mediaType === 'video') video = true
          else {
            let timer: NodeJS.Timeout | undefined
            video = await Promise.race([b.pip.hasVideo(wc), new Promise<boolean>((resolve) => (timer = setTimeout(() => resolve(false), 100)))])
            clearTimeout(timer)
          }
        }
        if (wc.isDestroyed()) return
        const template: MenuItemConstructorOptions[] = []
        if (params.linkURL) {
          const link = params.linkURL
          template.push({ label: t('menu.page.openInNewTab'), icon: menuIcon('openInNewTab'), click: () => openNext(link) })
          // Peek and split view live in the main window: not from a private window or an app.
          if (!priv && !inApp()) {
            template.push(
              { label: t('menu.page.openInPeek'), click: () => b.peek.open(link, tabId, { x: params.x, y: params.y }) },
              { label: t('menu.page.openInSplit'), icon: menuIcon('split'), click: () => b.splitView.openInSplit(link) }
            )
          }
          template.push(
            { label: t('menu.page.openInPrivate'), icon: menuIcon('private'), click: () => b.privates.open(link) },
            { label: t('menu.page.copyLink'), icon: menuIcon('link'), click: () => void clipboard.writeText(link) }
          )
        } else if (params.mediaType === 'image') {
          template.push(
            { label: t('menu.page.openImageInNewTab'), click: () => openNext(params.srcURL) },
            { label: t('menu.page.copyImage'), icon: menuIcon('copy'), click: () => wc.copyImageAt(params.x, params.y) },
            { label: t('menu.page.saveImage'), icon: menuIcon('download'), click: () => wc.downloadURL(params.srcURL) }
          )
        } else if (params.isEditable) {
          // Misspelled word: spellchecker suggestions first, then "Add to dictionary".
          const word = params.misspelledWord
          if (word) {
            const suggestions = params.dictionarySuggestions.slice(0, 5)
            if (suggestions.length) for (const s of suggestions) template.push({ label: s, click: () => wc.replaceMisspelling(s) })
            else template.push({ label: t('menu.page.noSuggestions'), enabled: false })
            template.push(
              { type: 'separator' },
              { label: t('menu.page.addToDictionary'), click: () => wc.session.addWordToSpellCheckerDictionary(word) },
              { type: 'separator' }
            )
          }
          template.push(
            { label: t('menu.edit.undo'), role: 'undo' },
            { label: t('menu.edit.redo'), role: 'redo' },
            { type: 'separator' },
            { label: t('menu.edit.cut'), role: 'cut' },
            { label: t('menu.edit.copy'), role: 'copy' },
            { label: t('menu.edit.paste'), role: 'paste' },
            { label: t('menu.edit.selectAll'), role: 'selectAll' }
          )
        } else if (params.selectionText.trim()) {
          const text = params.selectionText.trim()
          const excerpt = text.length > 30 ? `${text.slice(0, 30)}…` : text
          template.push(
            { label: t('menu.edit.copy'), role: 'copy' },
            { label: t('menu.page.search', { text: excerpt }), icon: menuIcon('find'), click: () => openNext(engineSearchUrl(text)) }
          )
        } else {
          template.push(
            { label: t('menu.page.back'), enabled: wc.navigationHistory.canGoBack(), click: () => wc.navigationHistory.goBack() },
            { label: t('menu.page.forward'), enabled: wc.navigationHistory.canGoForward(), click: () => wc.navigationHistory.goForward() },
            { label: t('menu.page.reload'), icon: menuIcon('reload'), click: () => wc.reload() }
          )
          // Page tools (web pages and files only).
          if (b.pageTools.canSave(tabId)) {
            const tab = info()
            template.push(
              { type: 'separator' },
              { label: t('menu.page.savePage'), click: () => void b.pageTools.savePage(tabId) },
              { label: t('menu.page.print'), click: () => b.pageTools.print(tabId) },
              { label: t('menu.page.capture'), icon: menuIcon('capture'), click: () => b.capture.start(tabId) }
            )
            if (b.translator.canTranslate(tabId)) {
              template.push({
                label: tab?.translation ? t('menu.page.showOriginal') : t('menu.page.translate', { language: b.translator.targetName() }),
                icon: menuIcon('translate'),
                click: () => b.translator.toggle(tabId)
              })
            }
            if (tab?.reader) {
              template.push({
                label: tab.reader === 'on' ? t('menu.page.exitReader') : t('menu.page.reader'),
                icon: menuIcon('reader'),
                click: () => b.reader.toggle(tabId)
              })
            }
            if (/^(https?|file):/i.test(wc.getURL())) template.push({ label: t('menu.page.viewSource'), click: () => b.pageTools.viewSource(tabId) })
          }
        }
        if (!priv && /^https?:\/\//i.test(wc.getURL())) {
          template.push({ type: 'separator' }, { label: t('menu.page.customizeSite'), click: () => b.boosts.open(siteOf(wc.getURL())) })
          // "Open on my phone": main window tabs, excluding link, image and input field.
          if (!b.store.detached.has(tabId) && !params.linkURL && params.mediaType !== 'image' && !params.isEditable) template.push(...b.link.menuItems(tabId))
          // Install the site as an app (or open it in the one that already covers it): from a main window tab.
          if (IS_MAC && !b.store.detached.has(tabId) && !params.linkURL && params.mediaType !== 'image' && !params.isEditable) {
            const installed = b.webApps.appFor(wc.getURL())
            template.push(
              installed
                ? { label: t('menu.page.openInApp', { name: installed.name }), click: () => b.webApps.moveToApp(tabId, installed.id) }
                : { label: t('menu.page.installApp'), click: () => void b.webApps.promptInstall(tabId) }
            )
          }
        }
        if (video) template.push({ type: 'separator' }, { label: t('menu.page.pip'), icon: menuIcon('pip'), click: () => void b.pip.open(tabId) })
        template.push({ type: 'separator' }, { label: t('menu.page.inspect'), click: () => b.devtools.inspect(tabId, params.x, params.y) })
        // Entries added by extensions, before "Inspect".
        const extra = b.extensions.menuItems(wc, params)
        const entries: (MenuItemConstructorOptions | MenuItem)[] = [...template]
        if (extra.length) entries.splice(entries.length - 2, 0, { type: 'separator' }, ...extra)
        buildMenu(entries).popup()
      })()
  )

  return view
}

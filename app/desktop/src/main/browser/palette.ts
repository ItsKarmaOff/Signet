import { app, net } from 'electron'
import { IS_MAC, setDefaultBrowser } from '../app/platform'
import type { PaletteMode, PaletteResult } from '@shared/types'
import { APP_ICONS } from '@shared/types'
import { iconPreview, setAppIcon } from '../app/appicon'
import type { Browser } from './browser'
import { boostActions } from '../features/boost/boost'
import { availableSources } from '../features/importers'
import { itemTabIds } from '../core/store'
import { engineName, engineSearchUrl, engineSuggestions, fold, toUrl } from '../core/urls'
import { parseSuggestions } from '@signet/shared/mobile/search'
import { resolveAlias } from '@shared/devhosts'
import { internalSuggestions } from '../web/internal'
import { t } from '../app/i18n'
import { shortcutLabel } from '@shared/shortcuts'
import { historyScore, matchQuality } from '../core/rank'

const MAX_RESULTS = 12
/** History and archive rows together; open tabs are not capped. */
const MAX_PAGES = 5

// Relevance of the fixed rows. Matches score 0-100 (see core/rank.ts): the search row sits above weak
// matches (generic word, site seen once long ago) and below strong ones (a site you visit, an open tab).
const SCORE = { internal: 2000, alias: 1500, typedUrl: 900, aliasPrefix: 115, tabBonus: 10, search: 45, action: 40, archive: 0.4 }

interface ActionDef {
  id: string
  title: string
  subtitle?: string
  shortcut?: string
  image?: string
  run: () => void
}

function buildActions(b: Browser, query = ''): ActionDef[] {
  const { state } = b.store
  const active = b.model.activeTab()
  const loc = active ? b.model.locate(active.id) : null
  const pinned = loc?.zone === 'pinned'
  const favorite = loc?.zone === 'favorites'
  const id = active?.id ?? null
  const peekLinks = state.settings.peekExternalLinks
  /** Current shortcut of a command (editable in Settings), or undefined. */
  const key = (command: string) => shortcutLabel(command, state.settings.shortcuts, process.platform === 'darwin')
  const actions: ActionDef[] = [
    { id: 'settings', title: t('palette.action.settings'), shortcut: '⌘,', run: () => b.settings.open() },
    { id: 'library', title: t('palette.action.library'), shortcut: key('library'), run: () => b.library.open() },
    { id: 'new-space', title: t('palette.action.newSpace'), run: () => b.spaces.edit(null) },
    { id: 'split', title: t('palette.action.splitView'), shortcut: key('openSplit'), run: () => b.overlay.openPalette('split') },
    {
      id: 'unsplit',
      title: t('palette.action.unsplit'),
      run: () => loc?.item.kind === 'split' && b.splitView.unsplit(loc.item.id)
    },
    ...(loc?.item.kind === 'split'
      ? (() => {
          const split = loc.item
          return [
            { id: 'split-columns', title: t('palette.action.splitColumns'), run: () => b.splitView.setLayout(split.id, 'columns') },
            { id: 'split-rows', title: t('palette.action.splitRows'), run: () => b.splitView.setLayout(split.id, 'rows') },
            ...(split.tabIds.length >= 3
              ? [{ id: 'split-grid', title: t('palette.action.splitGrid'), run: () => b.splitView.setLayout(split.id, 'grid') }]
              : []),
            { id: 'split-equalize', title: t('palette.action.splitEqualize'), run: () => b.splitView.setSizes(split.id, []) }
          ]
        })()
      : []),
    { id: 'toggle-sidebar', title: t('palette.action.toggleSidebar'), shortcut: key('toggleSidebar'), run: () => b.shell.toggleSidebar() },
    { id: 'compact', title: t('palette.action.compact'), shortcut: key('compactMode'), run: () => b.shell.toggleCompact() },
    {
      id: 'tab-layout',
      title: b.layout.horizontal ? t('palette.action.tabsSide') : t('palette.action.tabsTop'),
      shortcut: key('tabLayout'),
      run: () => b.shell.toggleTabLayout()
    },
    {
      id: 'pin',
      title: pinned || favorite ? t('palette.action.unpin') : t('palette.action.pin'),
      shortcut: key('pinTab'),
      run: () => id && b.arrange.togglePin(id)
    },
    {
      id: 'favorite',
      title: favorite ? t('palette.action.removeFavorite') : t('palette.action.addFavorite'),
      run: () => id && b.arrange.toggleFavorite(id)
    },
    { id: 'copy-url', title: t('palette.action.copyUrl'), shortcut: key('copyUrl'), run: () => b.tabs.copyUrl(id) },
    // Active phone: sends the page (a menu to choose if there are several).
    ...(id && b.link.canOpenOnPhone(id) ? [{ id: 'open-on-phone', title: t('palette.action.openOnPhone'), run: () => b.link.openOnPhoneChoose(id) }] : []),
    { id: 'little', title: t('palette.action.newLittleSignet'), shortcut: key('newLittle'), run: () => b.little.open('https://www.google.com') },
    { id: 'new-note', title: t('palette.action.newNote'), shortcut: key('newNote'), run: () => b.notes.create('note') },
    { id: 'new-easel', title: t('palette.action.newEasel'), shortcut: key('newEasel'), run: () => b.notes.create('easel') },
    { id: 'private', title: t('palette.action.newPrivateWindow'), shortcut: key('newPrivate'), run: () => b.privates.open() },
    // Sites installed as apps: the launcher is a macOS app, so Mac only.
    ...(IS_MAC && active && /^https?:\/\//i.test(active.url)
      ? [
          (() => {
            const installed = b.webApps.appFor(active.url)
            return installed
              ? { id: 'open-in-app', title: t('palette.action.openInApp', { name: installed.name }), run: () => b.webApps.moveToApp(active.id, installed.id) }
              : { id: 'install-app', title: t('palette.action.installApp'), shortcut: key('installApp'), run: () => void b.webApps.promptInstall(active.id) }
          })()
        ]
      : []),
    // Installed apps: open them from the command bar.
    ...b.webApps.names().map((a) => ({ id: `webapp:${a.id}`, title: t('palette.action.launchApp', { name: a.name }), run: () => b.webApps.launch(a.id) })),
    { id: 'pip', title: t('palette.action.pip'), run: () => void b.pip.open() },
    { id: 'clear-today', title: t('palette.action.archiveToday'), run: () => b.tabs.clearToday() },
    { id: 'close-tab', title: t('palette.action.closeTab'), shortcut: key('closeTab'), run: () => id && b.tabs.close(id) },
    { id: 'reopen', title: t('palette.action.reopenTab'), shortcut: key('reopenTab'), run: () => b.tabs.reopenClosed() },
    { id: 'reload', title: t('palette.action.reload'), shortcut: key('reload'), run: () => b.tabs.reload(id) },
    { id: 'devtools', title: t('palette.action.devTools'), shortcut: key('devTools'), run: () => b.devtools.toggle(id) },
    { id: 'js-console', title: t('palette.action.jsConsole'), shortcut: key('jsConsole'), run: () => b.devtools.showConsole(id) },
    // Developer mode: Develop menu commands for the active tab.
    ...(id && b.store.state.settings.devMode === true ? b.develop.paletteActions(id) : []),
    ...(id
      ? [
          { id: 'devtools-right', title: t('palette.action.devToolsRight'), run: () => b.devtools.setDock(id, 'right') },
          { id: 'devtools-bottom', title: t('palette.action.devToolsBottom'), run: () => b.devtools.setDock(id, 'bottom') },
          { id: 'devtools-window', title: t('palette.action.devToolsWindow'), run: () => b.devtools.setDock(id, 'window') }
        ]
      : []),
    ...(b.pageTools.canSave(id)
      ? [
          { id: 'print', title: t('palette.action.print'), shortcut: key('print'), run: () => b.pageTools.print(id) },
          { id: 'save-pdf', title: t('palette.action.savePdf'), shortcut: key('exportPdf'), run: () => void b.pageTools.savePdf(id) },
          { id: 'save-page', title: t('palette.action.savePage'), shortcut: key('savePage'), run: () => void b.pageTools.savePage(id) },
          // After the command bar closes: the tool is placed on the page.
          { id: 'capture', title: t('palette.action.capture'), shortcut: key('capture'), run: () => setTimeout(() => b.capture.start(id), 140) }
        ]
      : []),
    ...(id && b.reader.canRead(id)
      ? [
          { id: 'view-source', title: t('palette.action.viewSource'), shortcut: key('viewSource'), run: () => b.pageTools.viewSource(id) },
          {
            id: 'reader',
            title: active?.reader === 'on' ? t('palette.action.exitReader') : t('palette.action.reader'),
            shortcut: key('reader'),
            run: () => b.reader.toggle(id)
          },
          {
            id: 'translate',
            title: active?.translation ? t('palette.action.showOriginal') : t('palette.action.translate', { language: b.translator.targetName() }),
            shortcut: key('translate'),
            run: () => b.translator.toggle(id)
          }
        ]
      : []),
    {
      id: 'toggle-peek-links',
      title: t('palette.action.peekLinks'),
      subtitle: peekLinks ? t('palette.subtitle.on') : t('palette.subtitle.off'),
      run: () => b.shell.togglePeekLinks()
    },
    {
      id: 'toggle-adblock',
      title: t('palette.action.adblock'),
      subtitle: state.settings.adblock ? t('palette.subtitle.on') : t('palette.subtitle.off'),
      run: () => b.shell.toggleAdblock()
    },
    {
      id: 'default-browser',
      title: t('palette.action.defaultBrowser'),
      subtitle: app.isPackaged ? undefined : t('palette.subtitle.packagedOnly'),
      run: () => setDefaultBrowser()
    }
  ]
  const iconQuery = fold(query)
  if (
    iconQuery &&
    (iconQuery.includes('icon') ||
      (iconQuery.length >= 3 && 'icone'.startsWith(iconQuery)) ||
      APP_ICONS.some((i) => fold(t(`appIcon.${i.id}`)).includes(iconQuery)))
  ) {
    for (const icon of APP_ICONS) {
      actions.push({
        id: `icon:${icon.id}`,
        title: t('palette.action.appIcon', { name: t(`appIcon.${icon.id}`) }),
        subtitle: state.settings.appIcon === icon.id ? t('palette.subtitle.current') : undefined,
        image: iconPreview(icon.id),
        run: () => setAppIcon(b, icon.id)
      })
    }
  }
  actions.push({ id: 'new-folder', title: t('palette.action.newFolder'), run: () => b.folders.create() })
  // After the command bar closes: the form opens in the overlay.
  actions.push({ id: 'new-live-folder', title: t('palette.action.newLiveFolder'), run: () => setTimeout(() => b.liveFolders.openForm(null), 140) })
  for (const src of availableSources())
    actions.push({ id: `import:${src.id}`, title: t(src.titleKey, { name: src.name }), run: () => b.settings.openImport(src.id) })
  for (const space of state.spaces) {
    actions.push({ id: `space:${space.id}`, title: t('palette.action.goToSpace', { name: space.name }), run: () => b.spaces.switchTo(space.id) })
  }
  actions.push(...boostActions(b))
  return actions
}

function actionResult(a: ActionDef): PaletteResult {
  return { kind: 'action', id: `action:${a.id}`, title: a.title, subtitle: a.subtitle, action: a.id, shortcut: a.shortcut, image: a.image }
}

/** Open tabs (excluding Peek and Little Signet) with the name of their space. */
function openTabs(b: Browser): { id: string; space: string }[] {
  const { state } = b.store
  return [
    ...state.favorites.map((id) => ({ id, space: t('palette.favorites') })),
    ...state.spaces.flatMap((s) => [...s.pinned, ...s.today].flatMap(itemTabIds).map((id) => ({ id, space: s.name })))
  ]
}

// Folded keys of the archive entries, computed once per entry.
const archiveKeys = new WeakMap<object, { title: string; url: string }>()
function archiveKey(e: { title: string; url: string }): { title: string; url: string } {
  let k = archiveKeys.get(e)
  if (!k) {
    k = { title: fold(e.title), url: fold(e.url) }
    archiveKeys.set(e, k)
  }
  return k
}

/** Parsed web address, or null. */
function webUrl(url: string | undefined): URL | null {
  if (!url) return null
  try {
    const u = new URL(url)
    return u.protocol === 'http:' || u.protocol === 'https:' ? u : null
  } catch {
    return null
  }
}

/** Root of a site: path `/` (or empty), no query, no fragment. */
const isRootUrl = (u: URL): boolean => u.pathname === '/' && !u.search && !u.hash

/** Host without `www.`, the identity of a site for the root promotion. */
const siteKey = (u: URL): string => u.host.toLowerCase().replace(/^www\./, '')

/**
 * Chrome-like host promotion: when the best row is a deep page matched by the start of its host, the
 * site's root comes first instead, so the row, the inline completion (host only) and Enter all mean the
 * same thing. The root is an existing row (open tab, history, archive), else a history entry looked up
 * directly, else a synthesized row. `rows` is sorted best first; returns the list with the root first.
 */
function promoteRoot(b: Browser, fq: string, rows: PaletteResult[]): PaletteResult[] {
  const top = rows[0]
  if (!top || (top.kind !== 'tab' && top.kind !== 'history' && top.kind !== 'archived')) return rows
  const deep = webUrl(top.url)
  if (!deep || isRootUrl(deep) || matchQuality(fq, '', fold(top.url ?? '')) !== 100) return rows
  const site = siteKey(deep)
  const existing = rows.findIndex((r) => {
    if (r.kind !== 'tab' && r.kind !== 'history' && r.kind !== 'archived') return false
    const u = webUrl(r.url)
    return !!u && isRootUrl(u) && siteKey(u) === site
  })
  if (existing > 0) return [rows[existing], ...rows.slice(0, existing), ...rows.slice(existing + 1)]
  // Not among the candidates (the history search keeps only a few): look the root up directly.
  const www = /^www\./i.test(deep.hostname) ? '' : 'www.'
  const bare = deep.hostname.replace(/^www\./i, '')
  const hosts = [deep.hostname, www ? `${www}${bare}` : bare]
  const schemes = [deep.protocol, deep.protocol === 'https:' ? 'http:' : 'https:']
  const port = deep.port ? `:${deep.port}` : ''
  let entry: ReturnType<Browser['history']['get']>
  for (const scheme of schemes) for (const host of hosts) entry ??= b.history.get(`${scheme}//${host}${port}/`)
  const root = entry
    ? ({ kind: 'history', id: `history:${entry.url}`, title: entry.title, subtitle: entry.url, url: entry.url, favicon: entry.favicon } as const)
    : ({
        kind: 'history',
        id: `history:${deep.protocol}//${deep.host}/`,
        title: deep.host.replace(/^www\./i, ''),
        subtitle: `${deep.protocol}//${deep.host}/`,
        url: `${deep.protocol}//${deep.host}/`,
        favicon: top.favicon
      } as const)
  return [root, ...rows.filter((r) => r.id !== root.id)]
}

export function paletteQuery(b: Browser, query: string, mode: PaletteMode): PaletteResult[] {
  const { state } = b.store
  const q = query.trim()
  const tabResult = (id: string, subtitle: string): PaletteResult => ({
    kind: 'tab',
    id: `tab:${id}`,
    title: state.tabs[id].customTitle || state.tabs[id].title || state.tabs[id].url,
    subtitle,
    tabId: id,
    url: state.tabs[id].url,
    favicon: state.tabs[id].favicon
  })

  if (!q) {
    if (mode === 'edit-url') return []
    const space = b.space
    const recent = [...space.pinned, ...space.today]
      .flatMap(itemTabIds)
      .filter((id) => id !== space.activeTabId || mode === 'new-tab')
      .sort((x, y) => state.tabs[y].lastActive - state.tabs[x].lastActive)
      .slice(0, 6)
      .map((id) => tabResult(id, t('palette.goToTab', { space: space.name })))
    if (mode !== 'new-tab') return recent
    const wanted = ['split', 'little', 'reopen', 'toggle-sidebar']
    const actions = buildActions(b).filter((a) => wanted.includes(a.id))
    return [...recent, ...actions.map(actionResult)]
  }

  // Every candidate gets a relevance score; the list is sorted on it (stable: ties keep insertion order).
  const scored: { r: PaletteResult; s: number }[] = []
  const add = (r: PaletteResult, s: number) => scored.push({ r, s })
  // Input starting with `signet:`: the matching internal addresses come before everything else.
  for (const r of internalSuggestions(q)) add(r, SCORE.internal)
  // Local names: the typed name (with a possible path) opens its target; names starting with the input are offered.
  const aliases = state.settings.devAliases ?? []
  const aliased = resolveAlias(q, aliases)
  if (aliased) add({ kind: 'url', id: `url:${aliased}`, title: q.trim(), subtitle: t('palette.openAlias', { url: aliased }), url: aliased }, SCORE.alias)
  for (const a of aliases)
    if (a.name.startsWith(q.trim().toLowerCase()) && !aliased?.startsWith(a.target))
      add({ kind: 'url', id: `alias:${a.name}`, title: a.name, subtitle: t('palette.openAlias', { url: a.target }), url: a.target }, SCORE.aliasPrefix)
  // An address-like input is a navigation; free text is a search, which strong history matches may outrank.
  const url = aliased ? null : toUrl(q)
  if (url) {
    if (!scored.some(({ r }) => r.kind === 'url' && r.url === url))
      add({ kind: 'url', id: `url:${url}`, title: url, subtitle: t('palette.openAddress'), url }, SCORE.typedUrl)
  } else if (!aliased)
    add({ kind: 'search', id: `search:${q}`, title: t('palette.searchWith', { query: q, engine: engineName() }), url: engineSearchUrl(q) }, SCORE.search)

  const fq = fold(q)
  const now = Date.now()
  const opened = openTabs(b)
  // The same address appears only once, tab, history or archive.
  const seen = new Set<string>()
  for (const { id, space } of opened) {
    const tab = state.tabs[id]
    const quality = Math.max(matchQuality(fq, fold(tab.customTitle ?? ''), fold(tab.url)), matchQuality(fq, fold(tab.title), fold(tab.url)))
    if (!quality) continue
    seen.add(tab.url)
    // "Switch to tab" beats the same page in history.
    add(tabResult(id, t('palette.goToTab', { space })), quality + SCORE.tabBonus)
  }
  for (const a of buildActions(b, q)) {
    const title = fold(a.title)
    // Actions stay below the search row: Enter on a word must not run a command by surprise.
    if (title.includes(fq)) add(actionResult(a), title.startsWith(fq) ? SCORE.action : SCORE.action - 10)
  }

  const openUrls = new Set(opened.map(({ id }) => state.tabs[id].url))
  for (const e of b.history.search(q, openUrls, MAX_RESULTS)) {
    if (seen.has(e.url)) continue
    seen.add(e.url)
    add(
      { kind: 'history', id: `history:${e.url}`, title: e.title, subtitle: e.url, url: e.url, favicon: e.favicon },
      historyScore(fq, { ...e, title: fold(e.title), url: fold(e.url) }, now)
    )
  }
  for (const [i, e] of state.archive.entries()) {
    const key = archiveKey(e)
    const quality = matchQuality(fq, key.title, key.url)
    if (quality && !seen.has(e.url)) {
      seen.add(e.url)
      add(
        {
          kind: 'archived',
          id: `archived:${e.id ?? `${e.archivedAt}:${i}`}`,
          title: e.title || e.url,
          subtitle: t('palette.archived', { url: e.url }),
          url: e.url,
          favicon: e.favicon
        },
        quality * SCORE.archive
      )
    }
  }
  let sorted = scored.sort((x, y) => y.s - x.s).map((x) => x.r)
  // Free text without a typed path: the inline completion only names the host, so Enter must open it too.
  if (!aliased && !url && !q.includes('/') && !/\s/.test(q) && !/^signet:/i.test(q)) sorted = promoteRoot(b, fq, sorted)
  // Like other browsers, only a few past pages: a busy site must not flood the list.
  let pages = 0
  const list = sorted.filter((r) => (r.kind !== 'history' && r.kind !== 'archived') || ++pages <= MAX_PAGES)
  // "Search what you typed" never drops below the second row: it follows the best match.
  const search = list.findIndex((r) => r.kind === 'search')
  if (search > 1) list.splice(1, 0, ...list.splice(search, 1))
  return list.slice(0, MAX_RESULTS)
}

export async function paletteSuggest(query: string): Promise<PaletteResult[]> {
  const q = query.trim()
  if (!q) return []
  try {
    const { id, url } = engineSuggestions(q)
    const res = await net.fetch(url)
    return parseSuggestions(id, await res.json(), q, 4).map((s) => ({
      kind: 'suggestion' as const,
      id: `suggestion:${s}`,
      title: s,
      url: engineSearchUrl(s)
    }))
  } catch {
    return []
  }
}

export function paletteExecute(b: Browser, result: PaletteResult, mode: PaletteMode): void {
  b.overlay.close()
  switch (result.kind) {
    case 'tab':
      if (mode === 'split') b.splitView.splitActiveWith(result.tabId)
      else b.tabs.activate(result.tabId)
      break
    case 'action':
      if (result.action.startsWith('icon:')) {
        const id = result.action.slice(5)
        if (APP_ICONS.some((i) => i.id === id)) setAppIcon(b, id)
        break
      }
      buildActions(b)
        .find((a) => a.id === result.action)
        ?.run()
      break
    default:
      b.tabs.openUrl(result.url, mode)
  }
}

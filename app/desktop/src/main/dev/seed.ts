import { app, nativeTheme } from 'electron'
import type { Item } from '@shared/types'
import type { Browser } from '../browser/browser'
import { newSpace, newTab, type Store } from '../core/store'

/** Sample favicon: colored dot with a letter (no network). */
function letterIcon(letter: string, hue: number): string {
  const svg = `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 32 32"><rect width="32" height="32" rx="8" fill="hsl(${hue} 60% 50%)"/><text x="16" y="22" font-size="18" font-family="sans-serif" font-weight="700" text-anchor="middle" fill="white">${letter}</text></svg>`
  return `data:image/svg+xml;utf8,${encodeURIComponent(svg)}`
}

/**
 * Development setup, before the window opens: SIGNET_THEME=light|dark forces the theme;
 * SIGNET_SEED=1 replaces the in-memory state with a sample state (only with --user-data-dir, never the normal profile).
 */
export function devSetup(store: Store): void {
  if (app.isPackaged) return
  const theme = process.env.SIGNET_THEME
  if (theme === 'light' || theme === 'dark') nativeTheme.themeSource = theme
  if (process.env.SIGNET_SEED !== '1' || !process.argv.some((a) => a.startsWith('--user-data-dir='))) return

  const state = store.state
  const make = (title: string, url: string, hue: number) => {
    const t = newTab(url, title)
    t.favicon = letterIcon(title[0].toUpperCase(), hue)
    state.tabs[t.id] = t
    return t.id
  }
  const tab = (id: string): Item => ({ kind: 'tab', tabId: id })

  state.tabs = {}
  const favorites = [
    make('Wikipédia', 'https://www.wikipedia.org/', 210),
    make('GitHub', 'https://github.com/', 260),
    make('Hacker News', 'https://news.ycombinator.com/', 25),
    make('Le Monde', 'https://www.lemonde.fr/', 0)
  ]
  const perso = newSpace({ name: 'Personnel', icon: '🏠', hue: 250 })
  perso.pinned = [
    {
      kind: 'folder',
      id: 'dossier-bleu',
      name: 'Lecture',
      open: true,
      hue: 215,
      items: [
        tab(make('Wikipédia · Le libre', 'https://fr.wikipedia.org/', 210)),
        tab(make('W3C', 'https://www.w3.org/', 200)),
        tab(make('Example Domain', 'https://example.com/', 150))
      ]
    },
    { kind: 'folder', id: 'dossier-rouge', name: 'Actualités', open: true, hue: 0, items: [tab(make('Le Monde · À la une', 'https://www.lemonde.fr/', 0))] },
    tab(make('Notes de version', 'https://github.com/electron/electron/releases', 270))
  ]
  const wiki = make('Wikipédia, l’encyclopédie libre', 'https://www.wikipedia.org/', 210)
  perso.today = [
    tab(wiki),
    tab(make('Hacker News', 'https://news.ycombinator.com/', 25)),
    tab(make('Example Domain', 'https://example.com/', 150)),
    tab(make('W3C — Standards du Web', 'https://www.w3.org/', 200)),
    tab(make('GitHub — Where software is built', 'https://github.com/', 260))
  ]
  perso.activeTabId = wiki
  // SIGNET_ACTIVE=<URL fragment>: active tab at startup (captures only).
  const wanted = process.env.SIGNET_ACTIVE
  const found = wanted ? Object.values(state.tabs).find((t) => t.url.includes(wanted)) : undefined
  if (found) perso.activeTabId = found.id
  // SIGNET_URL: replaces the active tab's address (captures only).
  if (process.env.SIGNET_URL) state.tabs[perso.activeTabId!].url = process.env.SIGNET_URL
  // SIGNET_SPLIT=columns|rows|grid (and SIGNET_SPLIT_N=2..4): the first today tabs form a split view.
  const splitLayout = process.env.SIGNET_SPLIT
  if (splitLayout === 'columns' || splitLayout === 'rows' || splitLayout === 'grid') {
    const n = Math.min(4, Math.max(2, Number(process.env.SIGNET_SPLIT_N) || 4))
    const tabIds = perso.today.splice(0, n).flatMap((i) => (i.kind === 'tab' ? [i.tabId] : []))
    perso.today.unshift({ kind: 'split', id: 'vue-partagee', tabIds, layout: splitLayout })
  }
  // SIGNET_SIDEBAR=0: sidebar hidden (captures only).
  if (process.env.SIGNET_SIDEBAR === '0') state.sidebarVisible = false
  // SIGNET_LAYOUT=horizontal: tabs on top; SIGNET_TABS=<n>: n today tabs in total; fake downloads with SIGNET_LAYOUT.
  if (process.env.SIGNET_LAYOUT === 'horizontal') {
    state.settings.tabLayout = 'horizontal'
    const now = Date.now()
    state.downloads = [
      {
        id: 'dl-1',
        filename: 'rapport-annuel-2026.pdf',
        path: '/tmp/rapport.pdf',
        url: 'https://example.com/r.pdf',
        status: 'progressing',
        receivedBytes: 3_200_000,
        totalBytes: 8_000_000,
        startedAt: now
      },
      {
        id: 'dl-2',
        filename: 'photos-vacances.zip',
        path: '/tmp/photos.zip',
        url: 'https://example.com/p.zip',
        status: 'completed',
        receivedBytes: 52_000_000,
        totalBytes: 52_000_000,
        startedAt: now - 60_000
      }
    ]
  }
  const wantTabs = Number(process.env.SIGNET_TABS) || 0
  const titles = [
    'Docs React',
    'MDN Web Docs',
    'Stack Overflow',
    'Wikipédia · Océan',
    'npm',
    'TypeScript',
    'Le Monde · Sport',
    'YouTube',
    'OpenStreetMap',
    'Electron — Releases',
    'CSS-Tricks',
    'Hacker News · Show',
    'Can I use',
    'W3C · CSS',
    'Lobsters',
    'GitHub · Issues',
    'Vite',
    'Rust Book',
    'Python docs',
    'Linux Kernel'
  ]
  for (let n = perso.today.length; n < wantTabs; n++) {
    const title = `${titles[n % titles.length]} ${Math.floor(n / titles.length) + 1}`
    perso.today.push(tab(make(title, 'https://example.com/', (n * 47) % 360)))
  }
  const travail = newSpace({ name: 'Travail', icon: '💼', hue: 28 })
  state.spaces = [perso, travail]
  // SIGNET_SPACES=<n>: extra empty spaces (icon row by pages); SIGNET_ACTIVE_SPACE=<i>: active space.
  const extra: [string, string][] = [
    ['Lecture', '📚'],
    ['Sport', '🚴'],
    ['Cuisine', '🍳'],
    ['Voyages', '✈️'],
    ['Musique', '🎧'],
    ['Projets', '🧪'],
    ['Jeux', '🎮'],
    ['Famille', '🌻'],
    ['Finances', '💶'],
    ['Photo', '📷']
  ]
  for (let n = 2; n < Number(process.env.SIGNET_SPACES ?? 0); n++) {
    const [name, icon] = extra[(n - 2) % extra.length]
    state.spaces.push(newSpace({ name, icon, hue: (n * 53) % 360 }))
  }
  state.activeSpaceId = (state.spaces[Number(process.env.SIGNET_ACTIVE_SPACE ?? 0)] ?? perso).id
  state.favorites = favorites
  // Batch D: renamed tabs (title only, title + emoji), isolated space and sample archives.
  const todayTab = (title: string) => perso.today.map((i) => (i.kind === 'tab' ? state.tabs[i.tabId] : undefined)).find((t) => t?.title === title)
  const hn = todayTab('Hacker News')
  if (hn) Object.assign(hn, { customTitle: 'Veille techno', customIcon: '📰' })
  const ex = todayTab('Example Domain')
  if (ex) ex.customIcon = '🌍'
  travail.profile = 'isolated'
  // Batch C: sample routing rules.
  state.settings.routes = [
    { id: 'route-gh', pattern: 'github.com', spaceId: travail.id },
    { id: 'route-notion', pattern: '*.notion.site', spaceId: travail.id },
    { id: 'route-wiki', pattern: 'wikipedia.org', spaceId: perso.id }
  ]
  const ago = (hours: number) => Date.now() - hours * 3_600_000
  const archived = (title: string, url: string, hue: number, hours: number) => ({
    title,
    url,
    favicon: letterIcon(title[0].toUpperCase(), hue),
    archivedAt: ago(hours)
  })
  state.archive = [
    archived('Électron — Documentation', 'https://www.electronjs.org/fr/docs', 200, 2),
    archived('HTML — Wikipédia', 'https://fr.wikipedia.org/wiki/Hypertext_Markup_Language', 215, 5),
    archived('Le Monde — Éditorial', 'https://www.lemonde.fr/idees/', 0, 27),
    archived('Wikipédia · Cyclisme', 'https://fr.wikipedia.org/wiki/Cyclisme', 210, 30),
    archived('Open Street Map', 'https://www.openstreetmap.org/', 140, 80),
    archived('Notes de version', 'https://github.com/electron/electron/releases', 270, 400)
  ]
}

/** Sample history (only with SIGNET_SEED=1 and --user-data-dir, never on the normal profile). */
export function seedHistory(b: Browser): void {
  if (process.env.SIGNET_SEED !== '1' || !process.argv.some((a) => a.startsWith('--user-data-dir='))) return
  const entries = (b.history as unknown as { entries: Map<string, { url: string; title: string; favicon?: string; visits: number; lastVisit: number }> })
    .entries
  entries.clear()
  const rows: [string, string, number, number][] = [
    ['Wikipédia, l’encyclopédie libre', 'https://www.wikipedia.org/', 210, 0.2],
    ['Hacker News', 'https://news.ycombinator.com/', 25, 0.6],
    ['GitHub — Where software is built', 'https://github.com/', 260, 1.5],
    ['Example Domain', 'https://example.com/', 150, 3],
    ['W3C — Standards du Web', 'https://www.w3.org/', 200, 7],
    ['Le Monde — À la une', 'https://www.lemonde.fr/', 0, 26],
    ['Wikipédia · Le libre', 'https://fr.wikipedia.org/', 210, 29],
    ['Électron — Documentation', 'https://www.electronjs.org/fr/docs', 200, 33],
    ['HTML — Wikipédia', 'https://fr.wikipedia.org/wiki/Hypertext_Markup_Language', 215, 52],
    ['Open Street Map', 'https://www.openstreetmap.org/', 140, 75],
    ['Notes de version', 'https://github.com/electron/electron/releases', 270, 420]
  ]
  // Insertion order follows recency: the oldest first.
  for (const [title, url, hue, hours] of [...rows].reverse()) {
    entries.set(url, { url, title, favicon: letterIcon(title[0].toUpperCase(), hue), visits: 1, lastVisit: Date.now() - hours * 3_600_000 })
  }
}

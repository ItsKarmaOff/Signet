// DEV ONLY: state-seeding route for screenshots and QA (`exp://.../--/dev/seed`).
// Actions: toast, seed, reset, lang-fr, lang-en, theme-dark, theme-light, theme-system, private-on, private-off, open (?url=), space (?i=), download (?url=), download-private (?url=), pw-seed, blocking-on, blocking-off, link-seed, link-pair (?url=), link-general-on, link-general-off, link-device-on (?i=), link-device-off (?i=), link-forget (?i=), link-send (?url=), clear-site-data.
// Outside development (`__DEV__` false), the route does nothing and redirects to the home screen.
import { useEffect } from 'react'
import { Redirect, useLocalSearchParams, useRouter } from 'expo-router'
import { createWorkspace, openTab } from '@signet/shared/mobile/workspace'
import type { Workspace } from '@signet/shared/mobile/workspace'
import type { StoredLink } from '@signet/shared/mobile/link'
import type { Bookmark, HistoryEntry } from '@signet/shared/types'
import { decodePairingUrl } from '@signet/shared/link/protocol'
import { forgetLink, freezeLinkManager, pairWithOffer, patchRuntime, sendOpenToComputer } from '@/features/link'
import { startDownload } from '@/features/library'
import { addCredential, loadVault } from '@/features/passwords/vault'
import { siteDataCleaner, siteDataCount } from '@/features/settings/dataCleaners'
import { t } from '@/lib/i18n'
import { useStore } from '@/state'
import { toast } from '@/ui'

const DAY = 86_400_000

function seedWorkspace(): Workspace {
  let ws = createWorkspace('Perso')
  const personal = ws.activeSpaceId
  ws = { ...ws, spaces: ws.spaces.map((s) => ({ ...s, icon: '🏠', hue: 250 })) }
  const open = (url: string, title: string, extra: Parameters<typeof openTab>[1] extends infer O ? Partial<O> : never = {}) => {
    ws = openTab(ws, { url, title, ...extra })
  }
  open('https://en.wikipedia.org/wiki/Web_browser', 'Web browser - Wikipedia', { zone: 'favorites', activate: false })
  open('https://github.com', 'GitHub', { zone: 'favorites', activate: false })
  open('https://www.apple.com', 'Apple', { zone: 'pinned', activate: false })
  open('https://news.ycombinator.com', 'Hacker News', { zone: 'pinned', activate: false })
  open('https://en.wikipedia.org/wiki/Swift_(programming_language)', 'Swift (programming language) - Wikipedia', { activate: false })
  open('https://developer.mozilla.org/en-US/', 'MDN Web Docs', { activate: false })
  open('https://www.lemonde.fr', 'Le Monde', { activate: false })
  // Two more spaces.
  const mk = (name: string, icon: string, hue: number) => {
    const before = new Set(ws.spaces.map((s) => s.id))
    useStore.setState({ workspace: ws })
    useStore.getState().addSpace({ name, icon, hue })
    ws = useStore.getState().workspace
    const id = ws.spaces.find((s) => !before.has(s.id))!.id
    open('https://www.theverge.com', 'The Verge', { spaceId: id, activate: false })
    open('https://www.figma.com', 'Figma', { spaceId: id, activate: false })
    return id
  }
  mk('Travail', '💼', 200)
  mk('Lectures', '📚', 20)
  return { ...ws, activeSpaceId: personal }
}

function seedHistory(): HistoryEntry[] {
  const now = Date.now()
  const rows: [string, string, number][] = [
    ['https://en.wikipedia.org/wiki/Web_browser', 'Web browser - Wikipedia', 0.02],
    ['https://github.com', 'GitHub', 0.1],
    ['https://www.apple.com', 'Apple', 0.3],
    ['https://news.ycombinator.com', 'Hacker News', 1.1],
    ['https://www.lemonde.fr', 'Le Monde - Actualités', 1.3],
    ['https://developer.mozilla.org/en-US/', 'MDN Web Docs', 2.2],
    ['https://www.theverge.com', 'The Verge', 3.1],
    ['https://www.figma.com', 'Figma', 5.4]
  ]
  return rows.map(([url, title, d], i) => ({ url, title, visits: 3 + i, lastVisit: now - d * DAY }))
}

function seedBookmarks(): Bookmark[] {
  return [
    { kind: 'link', id: 'b1', title: 'GitHub', url: 'https://github.com' },
    { kind: 'link', id: 'b2', title: 'Apple', url: 'https://www.apple.com' },
    {
      kind: 'folder',
      id: 'f1',
      name: 'Lectures',
      items: [
        { kind: 'link', id: 'b3', title: 'Hacker News', url: 'https://news.ycombinator.com' },
        { kind: 'link', id: 'b4', title: 'The Verge', url: 'https://www.theverge.com' }
      ]
    }
  ]
}

function run(action: string, params: { url?: string; i?: string }): string {
  const s = useStore.getState()
  switch (action) {
    case 'seed':
      useStore.setState({ workspace: seedWorkspace(), history: seedHistory(), bookmarks: seedBookmarks() })
      s.setOnboardingDone(true)
      return '/'
    case 'reset':
      useStore.setState({ workspace: createWorkspace(t('spaces.defaultName')), history: [], bookmarks: [], links: [] })
      s.resetSettings()
      s.setOnboardingDone(false)
      return '/'
    case 'lang-fr':
      s.setSettings({ language: 'fr' })
      return '/'
    case 'lang-en':
      s.setSettings({ language: 'en' })
      return '/'
    case 'theme-dark':
      s.setSettings({ theme: 'dark' })
      return '/'
    case 'theme-light':
      s.setSettings({ theme: 'light' })
      return '/'
    case 'theme-system':
      s.setSettings({ theme: 'system' })
      return '/'
    case 'private-on':
      s.setPrivateMode(true)
      if (!useStore.getState().workspace.priv.tabIds.length) s.openTab({ url: 'https://duckduckgo.com', title: 'DuckDuckGo', private: true })
      return '/'
    case 'private-off':
      s.setPrivateMode(false)
      return '/'
    case 'open':
      s.openTab({ url: params.url ?? 'https://en.wikipedia.org/wiki/Web_browser' })
      return '/'
    case 'space': {
      const sp = s.workspace.spaces[Number(params.i ?? 0)]
      if (sp) s.setActiveSpace(sp.id)
      return '/'
    }
    case 'download':
    case 'download-private':
      void startDownload(params.url ?? 'https://www.w3.org/WAI/ER/tests/xhtml/testfiles/resources/pdf/dummy.pdf', action === 'download-private')
      return '/'
    case 'pw-seed':
      // 3 demo logins in the password vault; the first one's id is written to the log (`[dev] pw-seed id=...`).
      void loadVault().then(async () => {
        const rows = [
          { origin: 'https://en.wikipedia.org', username: 'alex', password: 'demo-wiki-pass' },
          { origin: 'https://github.com', username: 'alex@example.com', password: 'demo-github-pass' },
          { origin: 'https://www.lemonde.fr', username: 'alex.m', password: 'demo-lemonde-pass' }
        ]
        for (const [i, r] of rows.entries()) {
          const m = await addCredential(r)
          if (i === 0) console.log(`[dev] pw-seed id=${m?.id}`)
        }
      })
      return '/'
    case 'blocking-on':
    case 'blocking-off':
      s.setSettings({ contentBlocking: action === 'blocking-on' })
      return '/'
    case 'clear-site-data':
      // Log: number of sites with data before and after the native clear (cookies, cache, storage).
      void (async () => {
        const before = await siteDataCount()
        await siteDataCleaner()?.()
        console.log(`[dev] clear-site-data records ${before} -> ${await siteDataCount()}`)
      })()
      return '/'
    case 'link-pair': {
      // Like the confirmation screen's button, without touching the screen. Log: `[dev] link-pair ...`.
      const d = decodePairingUrl(params.url ?? '', Date.now())
      if (!d.ok) console.log(`[dev] link-pair ${d.error}`)
      else void pairWithOffer(d.offer).then((r) => console.log(`[dev] link-pair ${JSON.stringify(r)}`))
      return '/settings/computers'
    }
    case 'link-general-on':
    case 'link-general-off':
      s.setSettings({ linkEnabled: action === 'link-general-on' })
      return '/settings/computers'
    case 'link-device-on':
    case 'link-device-off': {
      const link = s.links[Number(params.i ?? 0)]
      if (link) s.patchLink(link.linkId, { enabled: action === 'link-device-on' })
      return '/settings/computers'
    }
    case 'link-forget': {
      const link = s.links[Number(params.i ?? 0)]
      if (link) void forgetLink(link.linkId)
      return '/settings/computers'
    }
    case 'link-send': {
      const link = s.links[Number(params.i ?? 0)]
      if (link) void sendOpenToComputer(link.linkId, params.url ?? '').then((ok) => console.log(`[dev] link-send ${ok}`))
      return '/'
    }
    case 'link-seed': {
      // Demo computers for screenshots: connected, paused at home, offline, switch off, link no longer valid.
      freezeLinkManager()
      const mk = (n: number, name: string, extra: Partial<StoredLink> = {}): StoredLink => ({
        linkId: n.toString(16).padStart(16, '0'),
        desktop: { id: `demo-${n}`, name, platform: n === 4 ? 'windows' : n === 5 ? 'linux' : 'macos' },
        certSha256: 'A'.repeat(43),
        port: 47615,
        hosts: ['192.168.1.20'],
        last: [],
        enabled: true,
        peerPaused: false,
        invalid: false,
        pairedAt: Date.now(),
        ...extra
      })
      const links = [
        mk(1, 'MacBook de Christophe'),
        mk(2, 'iMac du bureau', { peerPaused: true }),
        mk(3, 'Mac mini'),
        mk(4, 'PC de Léa', { enabled: false }),
        mk(5, 'Vieux portable', { invalid: true })
      ]
      useStore.setState({ links, linksLoaded: true })
      patchRuntime(links[0]!.linkId, {
        connected: true,
        servers: [
          { url: 'http://localhost:5173', port: 5173, title: 'Vite + React', fromTab: true },
          { url: 'http://localhost:3000', port: 3000, title: 'Next.js' },
          { url: 'http://localhost:8080', port: 8080 }
        ]
      })
      patchRuntime(links[2]!.linkId, { connected: false, connecting: true })
      s.setSettings({ linkEnabled: true })
      s.setOnboardingDone(true)
      return '/settings/computers'
    }
    case 'toast':
      toast.show({ message: t('tabs.closed'), actionLabel: t('common.undo'), onAction: () => {} })
      return '/'
    default:
      return '/'
  }
}

export default function DevAction() {
  const { action, url, i } = useLocalSearchParams<{ action: string; url?: string; i?: string }>()
  const router = useRouter()
  useEffect(() => {
    if (!__DEV__) return
    const to = run(String(action), { url, i })
    router.replace(to as '/')
  }, [action, url, i, router])
  return __DEV__ ? null : <Redirect href="/" />
}

import { StrictMode, type ComponentType } from 'react'
import { createRoot } from 'react-dom/client'
import { I18nextProvider } from 'react-i18next'
import { createI18n } from '@signet/shared/i18n'
import type { Locale } from '@signet/shared/i18n'
import type { Appearance } from '@shared/types'
import enUi from '@signet/shared/locales/en/ui.json'
import './styles/index.css'
import { Little } from './windows/Little'
import { Find } from './overlay/Find'
import { QuitHud } from './overlay/QuitHud'
import { ToastWindow } from './overlay/Toast'
import { Recovery, reportUiError } from './windows/Recovery'

// Without a bridge script (`preload` unreadable at launch, e.g. during a build), nothing can work:
// we reload a bit later rather than crash silently (the main process reloads too, see guard.ts).
if (!window.signet) {
  setTimeout(() => location.reload(), 1500)
  throw new Error('preload absent : rechargement')
}

// One class per route sets the backgrounds (see styles/).
const ROUTES: Record<string, string> = {
  '#overlay': 'overlay',
  '#little': 'little',
  '#find': 'find',
  '#sidebar': 'sidebar',
  '#toolbar': 'toolbar',
  '#boost': 'boost',
  '#settings': 'settings',
  '#welcome': 'welcome',
  '#library': 'library',
  '#pip': 'pip',
  '#quit': 'quit',
  '#toast': 'toast',
  '#private': 'private',
  '#preview': 'preview',
  '#webapp': 'webapp',
  '#devtools': 'devtools'
}
// Notes and Easels: tabs opened on `#note/<id>` or `#easel/<id>` (see src/main/features/notes/notes.ts).
const doc = /^#(note|easel)\//.exec(location.hash)?.[1]
const route = doc ?? ROUTES[location.hash] ?? 'main'
document.documentElement.classList.add(`route-${route}`)

// Appearance (tint amount, density, text size, transparency): attributes on <html>, read by styles/.
let lookPushed = false
const applyLook = (a: Appearance) => {
  const d = document.documentElement.dataset
  d.tint = a.tint
  d.density = a.density
  d.text = a.text
  d.glass = a.glass ? 'on' : 'off'
}
window.signet.onAppearance?.((a) => ((lookPushed = true), applyLook(a)))
void Promise.resolve()
  .then(() => window.signet.getAppearance())
  .then((a) => !lookPushed && applyLook(a))
  .catch(() => {})

// Rare routes are separate chunks: each window loads only its own.
const LAZY: Record<string, () => Promise<ComponentType>> = {
  main: () => import('./App').then((m) => m.App),
  sidebar: () => import('./App').then((m) => m.FloatingSidebar),
  toolbar: () => import('./App').then((m) => m.FloatingToolbar),
  overlay: () => import('./overlay/Overlay').then((m) => m.Overlay),
  boost: () => import('./overlay/Boost').then((m) => m.Boost),
  settings: () => import('./settings/Settings').then((m) => m.Settings),
  welcome: () => import('./windows/Welcome').then((m) => m.Welcome),
  library: () => import('./windows/Library').then((m) => m.Library),
  pip: () => import('./windows/Pip').then((m) => m.Pip),
  private: () => import('./windows/Private').then((m) => m.Private),
  note: () => import('./notes/Note').then((m) => m.Note),
  easel: () => import('./notes/Easel').then((m) => m.Easel),
  preview: () => import('./tabs/TabPreview').then((m) => m.TabPreview),
  webapp: () => import('./windows/WebApp').then((m) => m.WebApp),
  devtools: () => import('./devtools/DevToolsCard').then((m) => m.DevToolsWindow)
}
const STATIC: Record<string, ComponentType> = {
  little: Little,
  find: Find,
  quit: QuitHud,
  toast: ToastWindow
}
// Errors outside rendering (event handlers, promises): only logged.
window.addEventListener('error', (e) => reportUiError(route, e.error ?? e.message))
window.addEventListener('unhandledrejection', (e) => reportUiError(route, e.reason))

// Loaded in parallel with the language: the view renders only once ready.
const viewReady: Promise<ComponentType> = LAZY[route] ? LAZY[route]() : Promise.resolve(STATIC[route])

// `ui` catalog: English is bundled (fallback), French is a chunk loaded on demand.
const loadUi = (l: Locale) =>
  l === 'fr'
    ? import('@signet/shared/locales/fr/ui.json').then(
        (m) => m.default,
        () => enUi
      )
    : Promise.resolve(enUi)

// The view only appears once the language is known (no flicker); later changes are applied live.
let latest: Locale | null = null
let apply: ((l: Locale) => void) | null = null
// Safeguard: with a preload older than the UI (development), the view is still shown, in English.
window.signet.onLocale?.((l) => (apply ? apply(l) : (latest = l)))
void Promise.resolve()
  .then(() => window.signet.getLocale())
  .catch((): Locale => 'en')
  .then(async (first) => {
    const locale = latest ?? first
    const [View, catalog] = await Promise.all([viewReady, loadUi(locale)])
    const i18n = createI18n(locale, 'ui', { en: { ui: enUi }, [locale]: { ui: catalog } })
    document.documentElement.lang = locale
    let seq = 0
    apply = (l) => {
      const mine = ++seq
      void loadUi(l).then((c) => {
        if (mine !== seq) return // a more recent change wins
        document.documentElement.lang = l
        i18n.addResourceBundle(l, 'ui', c, true, true)
        return i18n.changeLanguage(l)
      })
    }
    // A language change that arrived during loading.
    if (latest && latest !== locale) apply(latest)
    try {
      sessionStorage.removeItem('signet-boot-retries') // startup succeeded
    } catch {
      // not important
    }
    createRoot(document.getElementById('root')!).render(
      <StrictMode>
        <I18nextProvider i18n={i18n}>
          <Recovery route={route}>
            <View />
          </Recovery>
        </I18nextProvider>
      </StrictMode>
    )
  })
  // Startup impossible (UI chunk unreadable, e.g. during an update): log, then retry.
  .catch((error: unknown) => {
    reportUiError(route, error, 'startup')
    // Retry, increasingly spaced (1.5 s, 3 s, 6 s… 30 s at most): the view eventually comes back as soon as the code is
    // readable again, without ever staying empty (an empty, transparent view is invisible).
    const tries = (() => {
      try {
        const n = Number(sessionStorage.getItem('signet-boot-retries')) || 0
        sessionStorage.setItem('signet-boot-retries', String(n + 1))
        return n
      } catch {
        return 0
      }
    })()
    setTimeout(() => location.reload(), Math.min(30_000, 1500 * 2 ** Math.min(tries, 5)))
  })

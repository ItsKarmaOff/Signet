import { app, webContents, WebContentsView, type Session } from 'electron'
import { join } from 'node:path'
import { extensionApisPath } from '../../../core/urls'
import { Alarms } from './alarms'
import { privacySettings, applyWebRtcTo, WEBRTC_KEY } from './privacy'
import { proxySetting } from './proxy'
import { ApiRouter } from './router'
import { backgroundContext, filterContexts, pageContext, type ExtensionContext } from './contexts'
import { DnrStore } from './dnr'
import { Offscreen, type OffscreenPage } from './offscreen'
import { ProxyAuthBroker } from './auth'
import { ChromeSettings, type SettingDef } from './settings'
import { AUTH_LISTENERS_METHOD, AUTH_RESPONSE_METHOD, INSTALLED_METHOD, SETTING_METHODS, type InstallReport } from '@shared/extension-apis'
import { extensionOfPage } from './router'

/**
 * Extension APIs that Electron and electron-chrome-extensions lack: `chrome.proxy`, `chrome.privacy`, `chrome.alarms`.
 * One instance for the app; `attach(ses)` for each regular session (never private ones, which have no extensions).
 * Later phases add their API as one more file registering on the router.
 */
export class ExtensionApis {
  readonly router = new ApiRouter()
  private settings: ChromeSettings | null = null
  private alarms: Alarms | null = null
  private sessions = new Set<Session>()
  private dnr: DnrStore | null = null
  private offscreen = new Offscreen(
    (ses) => openOffscreenPage(ses as Session),
    (m) => console.log(m)
  )

  /** Persistent state is created on first use: `userData` is only reliable once the app is ready. */
  private init(): { settings: ChromeSettings; alarms: Alarms } {
    if (!this.settings || !this.alarms) {
      const dir = app.getPath('userData')
      const defs: SettingDef[] = [proxySetting, ...privacySettings()]
      const settings = (this.settings = new ChromeSettings(join(dir, 'extension-settings.json'), defs, this.router, () => this.sessions))
      this.alarms = new Alarms(join(dir, 'extension-alarms.json'), this.router)
      for (const def of defs) for (const m of SETTING_METHODS) this.registerSetting(this.settings, def, m)
      for (const [method, fn] of Object.entries(this.alarms.methods())) this.router.register('alarms', method, fn)
      this.dnr = new DnrStore(join(dir, 'extension-dnr.json'), (m) => !app.isPackaged && console.log(m))
      for (const [method, fn] of Object.entries(this.dnr.methods()))
        this.router.register(['declarativeNetRequest', 'declarativeNetRequestWithHostAccess'], method, fn)
      this.router.register('offscreen', 'offscreen.createDocument', ({ ses, extensionId }, params) => this.offscreen.create(ses, extensionId, params))
      this.router.register('offscreen', 'offscreen.closeDocument', ({ extensionId }) => this.offscreen.close(extensionId))
      this.router.register('offscreen', 'offscreen.hasDocument', ({ extensionId }) => this.offscreen.has(extensionId))
      this.initAuth(settings)
      this.router.register('', 'runtime.getContexts', ({ ses, extensionId }, filter) => filterContexts(this.contextsOf(ses, extensionId), filter))
      this.router.register('', INSTALLED_METHOD, ({ extensionId }, report) => this.logInstalled(extensionId, report as InstallReport))
      app.on('will-quit', () => this.flush())
      // Tabs opened after the WebRTC policy was set must follow it.
      app.on('web-contents-created', (_e, wc) => {
        if (this.sessions.has(wc.session) && this.settings?.controllerOf(WEBRTC_KEY)) applyWebRtcTo(wc, this.settings.effective(WEBRTC_KEY))
        // Development diagnostic: why a page fails while an extension's proxy is on (no session.webRequest handler, which
        // would silence the extensions' own listeners).
        if (!app.isPackaged) {
          wc.on('did-fail-load', (_ev, code, description, url) => {
            if (code !== -3 && this.sessions.has(wc.session) && this.settings?.controllerOf('proxy.settings')) {
              console.log(`[extension-apis] load failed ${code} ${description} ${url.slice(0, 160)}`)
            }
          })
        }
      })
    }
    return { settings: this.settings, alarms: this.alarms }
  }

  /** Proxy authentication: Electron's `login` event for a proxy becomes `onAuthRequired` for the extension controlling the proxy. */
  private initAuth(settings: ChromeSettings): void {
    const broker = new ProxyAuthBroker({
      emitter: this.router,
      responder: () => {
        const id = settings.controllerOf('proxy.settings')
        return id && this.router.extensionsWith('webRequest').concat(this.router.extensionsWith('webRequestAuthProvider')).includes(id) ? id : undefined
      },
      log: (m) => console.log(m)
    })
    this.router.register(['webRequest', 'webRequestAuthProvider'], AUTH_RESPONSE_METHOD, ({ extensionId, context }, key, response) =>
      broker.respond(extensionId, context, key, response)
    )
    this.router.register(['webRequest', 'webRequestAuthProvider'], AUTH_LISTENERS_METHOD, ({ extensionId, context }, count) =>
      broker.setListeners(extensionId, context, Number(count))
    )
    app.on('login', (event, wc, details, info, callback) => {
      // Only proxy challenges, only from our regular sessions; website logins keep Electron's default.
      if (!broker.canHandle(info.isProxy) || (wc && !this.sessions.has(wc.session))) return
      event.preventDefault()
      if (!app.isPackaged) console.log(`[extension-apis] login event: proxy ${info.host}:${info.port} for ${details.url.slice(0, 120)}`)
      void broker
        .handle({ url: details.url, host: info.host, port: info.port, scheme: info.scheme, realm: info.realm ?? '' })
        .then((o) => (o.action === 'credentials' ? callback(o.username, o.password) : callback()))
    })
  }

  private registerSetting(settings: ChromeSettings, def: SettingDef, op: (typeof SETTING_METHODS)[number]): void {
    this.router.register(def.permission, `${def.key}.${op}`, ({ extensionId }, details) => {
      if (op === 'get') return settings.details(def.key, extensionId)
      if (op === 'set') return settings.set(def.key, extensionId, details)
      return settings.clear(def.key, extensionId)
    })
  }

  /** Contexts of an extension: its running worker and its open pages. Offscreen documents come with phase 2. */
  private contextsOf(ses: Session, extensionId: string): ExtensionContext[] {
    const contexts: ExtensionContext[] = []
    const background = (ses.extensions.getExtension(extensionId)?.manifest as { background?: { service_worker?: string } } | undefined)?.background
    const worker = background?.service_worker
    if (worker && this.router.runningWorkers(ses, extensionId)) contexts.push(backgroundContext(extensionId, `chrome-extension://${extensionId}/${worker}`))
    contexts.push(...this.offscreen.contexts(extensionId))
    for (const wc of webContents.getAllWebContents()) {
      if (!wc.isDestroyed() && !this.offscreen.owns(wc.id) && wc.session === ses && extensionOfPage(wc.getURL()) === extensionId)
        contexts.push(pageContext(extensionId, wc.id, wc.getURL()))
    }
    return contexts
  }

  private logInstalled(extensionId: string, r: InstallReport): void {
    if (!r || typeof r !== 'object') return
    const names = Array.isArray(r.namespaces) ? r.namespaces.map(String).join(', ') : ''
    const error = r.error ? ` ERROR ${String(r.error).slice(0, 200)}` : ''
    console.log(`[extension-apis] ${extensionId} ${r.context === 'page' ? 'page' : 'worker'}: ${names || 'none'} (${String(r.mode)})${error}`)
  }

  /**
   * Must run AFTER `new ElectronChromeExtensions` for the session: the library defines its own stubs (privacy, downloads...)
   * and freezes `chrome`; our preload registered later runs later and replaces those namespaces on a copy (see the preload).
   */
  attach(ses: Session): void {
    if (this.sessions.has(ses)) return
    this.sessions.add(ses)
    const { settings, alarms } = this.init()
    for (const type of ['service-worker', 'frame'] as const) {
      try {
        ses.registerPreloadScript({ id: `signet-extension-apis-${type}`, type, filePath: extensionApisPath })
      } catch (e) {
        console.error('Extension APIs: preload unavailable', e)
      }
    }
    this.router.attach(ses)
    ses.extensions.on('extension-loaded', (_e, ext) => {
      settings.reapply(ses, ext.id)
      alarms.scheduler.activate(ext.id)
    })
    ses.extensions.on('extension-unloaded', (_e, ext) => {
      // Gone from this session: its proxy must not keep applying here.
      if (settings.controllerOf('proxy.settings') === ext.id) void ses.setProxy({ mode: 'system' }).catch(() => {})
      // The extension is loaded in every session: it is only really gone once the last one lets go.
      if (this.router.loadedElsewhere(ext.id, ses)) return
      settings.release(ext.id)
      this.offscreen.closeFor(ext.id)
      this.dnr?.remove(ext.id)
      alarms.scheduler.remove(ext.id)
    })
  }

  /** The extension's `chrome.*` endpoints for pages of an extension (calls from `chrome-extension://` frames). */
  registerPageIpc(): void {
    this.router.registerPageIpc()
  }

  /** Writes pending state (app quit). */
  flush(): void {
    this.settings?.flush()
    this.alarms?.flush()
    this.dnr?.flush()
  }
}

/**
 * A hidden page for an offscreen document: a WebContentsView that is never added to a window, in the extension's session
 * with the same isolation as other extension pages (so the library's preload and ours both run). It is not registered
 * with the tab logic, so it cannot appear, take focus or become a tab.
 */
function openOffscreenPage(ses: Session): OffscreenPage {
  const view = new WebContentsView({ webPreferences: { session: ses, sandbox: true, contextIsolation: true, backgroundThrottling: false } })
  const wc = view.webContents
  const id = wc.id
  return {
    id,
    load: (url) => wc.loadURL(url),
    close: () => {
      if (!wc.isDestroyed()) wc.close()
    },
    onClosed: (fn) => wc.once('destroyed', fn)
  }
}

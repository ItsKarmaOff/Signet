import type { Session } from 'electron'
import type { Store } from '../core/store'
import { gpcPath } from '../core/urls'
import { NetworkOrder, networkHub } from './network'

const NAME = 'gpc'
const SCRIPT_ID = 'signet-gpc'

/**
 * Global Privacy Control (Settings › Privacy). While the setting is on, every http(s) request carries `Sec-GPC: 1` (a
 * consumer of the network hub on `onBeforeSendHeaders`) and a page script defines `navigator.globalPrivacyControl`.
 * Both exist only while the setting is on: Electron gives its own `session.webRequest` listeners precedence over the
 * extensions' `chrome.webRequest` ones, so holding `onBeforeSendHeaders` silences them, a price paid only by users who
 * turned this on. Workers do not see `navigator.globalPrivacyControl` (only the header applies to them).
 */
export class Gpc {
  private sessions = new Set<Session>()
  /** Sessions where the page script is registered. */
  private scripted = new WeakSet<Session>()

  constructor(private readonly d: { store: Pick<Store, 'state'> }) {}

  get enabled(): boolean {
    return this.d.store.state.settings.globalPrivacyControl === true
  }

  /** Adds a session (regular, isolated or private); GPC is active on it while the setting is on. */
  attach(ses: Session): void {
    this.sessions.add(ses)
    this.apply(ses)
  }

  /** Removes a session (private window closed). */
  detach(ses: Session): void {
    if (!this.sessions.delete(ses)) return
    networkHub(ses).release(NAME)
    this.scripted.delete(ses)
  }

  /** Registers or releases the header consumer and the page script on all sessions, depending on the setting. */
  sync(): void {
    for (const ses of this.sessions) this.apply(ses)
  }

  private apply(ses: Session): void {
    const hub = networkHub(ses)
    if (!this.enabled) {
      hub.release(NAME)
      this.unscript(ses)
      return
    }
    hub.on('onBeforeSendHeaders', NAME, NetworkOrder.headers, { urls: ['http://*/*', 'https://*/*'] }, (d, cb) => {
      cb({ requestHeaders: { ...d.requestHeaders, 'Sec-GPC': '1' } })
    })
    this.script(ses)
  }

  private script(ses: Session): void {
    if (this.scripted.has(ses)) return
    try {
      ses.registerPreloadScript({ id: SCRIPT_ID, type: 'frame', filePath: gpcPath })
      this.scripted.add(ses)
    } catch (e) {
      console.error('Global Privacy Control: page script unavailable', e)
    }
  }

  private unscript(ses: Session): void {
    if (!this.scripted.delete(ses)) return
    try {
      ses.unregisterPreloadScript(SCRIPT_ID)
    } catch (e) {
      console.error('Global Privacy Control: page script not removed', e)
    }
  }
}

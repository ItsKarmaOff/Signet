import { ipcMain, type WebContents } from 'electron'
import type { SignetApi } from '@shared/types'
import { IPC, PIP_IPC, type SignetPipApi, type InvokeMethod } from '@shared/ipc/contract'
import { firstInvalid } from '@shared/ipc/validate'
import { fromUi } from '../web/guard'
import type { Browser } from '../browser/browser'

/** What a handler receives besides the arguments: the sender (main window bar, a Little Signet…). */
export interface IpcCtx {
  sender: WebContents
}

type Args<K extends InvokeMethod> = Parameters<SignetApi[K]>
type Result<K extends InvokeMethod> = SignetApi[K] extends (...a: never[]) => Promise<infer R> ? R : never
/** A call that returns `void` may return anything (the result is not read by the interface). */
type Returned<K extends InvokeMethod> = void extends Result<K> ? unknown : Result<K> | PromiseLike<Result<K>>

export type InvokeHandler<K extends InvokeMethod> = (ctx: IpcCtx, ...args: Args<K>) => Returned<K>

type PipMethod = keyof SignetPipApi

export interface Registrar {
  /** Handler of a `SignetApi` call, by method name: arguments and result are typed from `SignetApi`. */
  handle<K extends InvokeMethod>(method: K, fn: InvokeHandler<K>, opts?: { onInvalid?: () => Result<K> }): void
  /** Handler of a mini player gesture (`window.signetPip`, no reply). */
  handlePip<K extends PipMethod>(method: K, fn: (ctx: IpcCtx, ...args: Parameters<SignetPipApi[K]>) => void): void
  /** Channels registered so far. */
  readonly channels: ReadonlySet<string>
}

/** One warning per channel: a repeated malformed call must not flood the log. */
const warned = new Set<string>()
function warnOnce(dev: boolean, channel: string, index: number): void {
  if (!dev || warned.has(channel)) return
  warned.add(channel)
  console.warn(`[ipc] ${channel}: invalid argument ${index}`)
}

/**
 * Registers the interface channels. Central guard: only the app's interface pages may call,
 * not a tab, nor a web page. Arguments are checked against the contract before calling the handler
 * (call refused with an error; send ignored).
 */
export function createRegistrar(b: Browser, dev: boolean): Registrar {
  const channels = new Set<string>()
  const claim = (channel: string) => {
    if (channels.has(channel)) throw new Error(`IPC channel already registered: ${channel}`)
    channels.add(channel)
  }
  const allowed = (e: Electron.IpcMainInvokeEvent | Electron.IpcMainEvent) => fromUi(e) && !b.views.tabIdOfContents(e.sender.id)

  return {
    channels,
    handle(method, fn, opts) {
      const def = IPC[method]
      claim(def.channel)
      ipcMain.handle(def.channel, (e, ...raw) => {
        if (!allowed(e)) throw new Error('IPC call refused')
        const bad = firstInvalid(def.args, raw)
        if (bad >= 0) {
          warnOnce(dev, def.channel, bad)
          if (opts?.onInvalid) return opts.onInvalid()
          throw new Error(`Invalid IPC call: ${def.channel}`)
        }
        return (fn as (ctx: IpcCtx, ...a: unknown[]) => unknown)({ sender: e.sender }, ...raw.slice(0, def.args.length))
      })
    },
    handlePip(method, fn) {
      const def = PIP_IPC[method]
      claim(def.channel)
      ipcMain.on(def.channel, (e, ...raw) => {
        if (!allowed(e)) return
        const bad = firstInvalid(def.args, raw)
        if (bad >= 0) return warnOnce(dev, def.channel, bad)
        ;(fn as (ctx: IpcCtx, ...a: unknown[]) => void)({ sender: e.sender }, ...raw.slice(0, def.args.length))
      })
    }
  }
}

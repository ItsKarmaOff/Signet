import { contextBridge, ipcRenderer } from 'electron'
import type { SignetApi } from '@shared/types'
import { IPC, PIP_IPC, type SignetPipApi } from '@shared/ipc/contract'
import { injectBrowserAction } from 'electron-chrome-extensions/browser-action'

/** Subscription to an event pushed by the main process; returns the unsubscribe function. */
function subscribe(channel: string, cb: (...args: unknown[]) => void): () => void {
  const listener = (_e: Electron.IpcRendererEvent, ...args: unknown[]) => cb(...args)
  ipcRenderer.on(channel, listener)
  return () => {
    ipcRenderer.removeListener(channel, listener)
  }
}

// `window.signet` is built from the contract (src/shared/ipc/contract.ts): a loop, no hand-written method.
// A call sends exactly the contract's arguments (the others are ignored); the main process checks them.
const api: Record<string, unknown> = {}
for (const [name, def] of Object.entries(IPC)) {
  api[name] =
    def.kind === 'event'
      ? (cb: (...args: unknown[]) => void) => subscribe(def.channel, cb)
      : (...args: unknown[]) => ipcRenderer.invoke(def.channel, ...args.slice(0, def.args.length))
}

contextBridge.exposeInMainWorld('signet', api as unknown as SignetApi)
// Mini-player gestures (route #pip only): fire-and-forget sends, also from the contract.
const pip: Partial<SignetPipApi> = {}
for (const [name, def] of Object.entries(PIP_IPC)) {
  ;(pip as Record<string, unknown>)[name] = (...args: unknown[]) => ipcRenderer.send(def.channel, ...args.slice(0, def.args.length))
}
contextBridge.exposeInMainWorld('signetPip', pip)

// Extension action buttons element: only for the main window and the compact mode's floating bar.
const { hash } = (globalThis as unknown as { location: { hash: string } }).location
if (hash === '' || hash === '#toolbar') injectBrowserAction()

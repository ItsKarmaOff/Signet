/// <reference lib="dom" />
// Page script for notes and Easels (registered on the tabs' sessions, all frames).
// It only activates in the main frame of an app interface page opened on a document's route
// (`#note/<id>`, `#easel/<id>`); the main process rechecks everything (tab, main frame, address) and takes the id from the address.
import { contextBridge, ipcRenderer } from 'electron'
import type { SignetNotesApi, NotesSignetSubset } from '@shared/notes'

const route = /^#(note|easel)\/[0-9a-f-]{36}$/i.test(location.hash)
const app =
  location.protocol === 'signetui:'
    ? location.host === 'app' && location.pathname === '/index.html'
    : /^http:\/\/(localhost|127\.0\.0\.1)(:\d+)?$/.test(location.origin) // dev server (the main process checks the exact address)

if (process.isMainFrame && route && app) {
  const subscribe =
    <T>(channel: string) =>
    (cb: (value: T) => void): (() => void) => {
      const listener = (_e: Electron.IpcRendererEvent, value: T) => cb(value)
      ipcRenderer.on(channel, listener)
      return () => {
        ipcRenderer.removeListener(channel, listener)
      }
    }
  // Interface language and appearance: what main.tsx needs to display the page.
  const signet: NotesSignetSubset = {
    getLocale: () => ipcRenderer.invoke('notes:locale'),
    onLocale: subscribe('locale'),
    getAppearance: () => ipcRenderer.invoke('notes:appearance'),
    onAppearance: subscribe('appearance')
  }
  const notes: SignetNotesApi = {
    load: () => ipcRenderer.invoke('notes:load'),
    save: (patch) => ipcRenderer.invoke('notes:save', patch),
    remove: () => ipcRenderer.invoke('notes:remove')
  }
  contextBridge.exposeInMainWorld('signet', signet)
  contextBridge.exposeInMainWorld('signetNotes', notes)
}

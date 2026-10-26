import type { BrowserWindow } from 'electron'
import { pushUi } from '../ipc/push'
import type { Store } from '../core/store'
import { isSidebarOn } from '../model/chrome'
import type { Changed } from './broadcast'
import { UiView } from './uiview'

/** Safeguard: if the notice page never signals that it is listening, its transparent view must not stay over the page. */
const TOAST_READY_MS = 15000

export type UndoKind = 'archived' | 'tabsClosed' | 'spaceDeleted'

export interface UndoDeps {
  store: Store
  window: { readonly win: BrowserWindow | null }
  changed: Changed
}

/**
 * "… · Undo" notices (archiving, group or folder closed, space deleted): only one is undoable at a time.
 * They are shown in the sidebar or, without a visible bar (tabs on top, compact…), in their own small view.
 */
export class Undo {
  /** Last undoable notice; `commit` makes the deletion final. */
  private last: { id: number; run: () => void; commit?: () => void } | null = null
  private seq = 0
  /** Floating notice view when no sidebar can display it. */
  private toastUi: UiView | null = null
  private toastTimer: ReturnType<typeof setTimeout> | null = null

  constructor(private readonly d: UndoDeps) {}

  /** The notice view, if it exists (placed by the layout). */
  get view() {
    return this.toastUi?.view ?? null
  }

  /** "… · Undo" notice: `run` undoes the action, `commit` makes it final (notice replaced or expired). */
  announce(kind: UndoKind, count: number, name: string, run: () => void, commit?: () => void): void {
    this.last?.commit?.()
    const id = ++this.seq
    this.last = { id, run, commit }
    const payload = { id, kind, count, name }
    if (isSidebarOn(this.d.store.state)) {
      if (this.d.window.win) pushUi(this.d.window.win.webContents, 'today:archived', payload)
      return
    }
    // No visible sidebar (tabs on top, compact…): the notice lives in its own small view.
    if (!this.toastUi) {
      this.toastUi = new UiView('#toast', true)
      const ui = this.toastUi
      if (this.toastTimer) clearTimeout(this.toastTimer)
      this.toastTimer = setTimeout(() => {
        this.toastTimer = null
        if (this.toastUi === ui) this.toastDone()
      }, TOAST_READY_MS)
      this.d.changed({ persist: false })
    }
    this.toastUi.send('today:archived', payload)
  }

  /** Undoes notice `id` if it is still the last one (click on "Undo"). */
  undo(id: number): void {
    const undo = this.last
    if (!undo || undo.id !== id) return
    this.last = null
    undo.run()
  }

  /** Closes the undo notice (UI side) when the undo window has passed. */
  expire(commit: () => void): void {
    const undo = this.last
    if (!undo || undo.commit !== commit) return
    this.last = null
    if (this.d.window.win) pushUi(this.d.window.win.webContents, 'today:undo-expired', undo.id)
    this.toastUi?.send('today:undo-expired', undo.id)
  }

  /** Signal from the notice view: its page is subscribed. False if the signal does not come from it. */
  markToastReady(sender: Electron.WebContents): boolean {
    if (!this.toastUi || sender !== this.toastUi.webContents) return false
    if (this.toastTimer) clearTimeout(this.toastTimer)
    this.toastTimer = null
    this.toastUi.markReady()
    return true
  }

  /** The notice has finished its exit: its view is removed. */
  toastDone(): void {
    if (this.toastTimer) clearTimeout(this.toastTimer)
    this.toastTimer = null
    if (!this.toastUi) return
    this.toastUi.destroy()
    this.toastUi = null
    this.d.changed({ persist: false })
  }

  /** Main window closed: the notice view goes away. */
  reset(): void {
    if (this.toastTimer) clearTimeout(this.toastTimer)
    this.toastTimer = null
    this.toastUi?.destroy()
    this.toastUi = null
  }
}

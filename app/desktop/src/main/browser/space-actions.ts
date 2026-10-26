import { dialog, type BrowserWindow, type WebContents } from 'electron'
import type { SpaceDraft } from '@shared/types'
import { pushUi } from '../ipc/push'
import type { Store } from '../core/store'
import { t } from '../app/i18n'
import type { Model } from '../model'
import { isHorizontal } from '../model/chrome'
import { clearPendingWipe, clearSpaceData, isIsolated, markPendingWipe } from '../web/profiles'
import type { Changed } from './broadcast'
import type { Layout } from './layout'
import type { Overlay } from './overlay'
import type { Undo } from './undo'
import type { Views } from './views'

/** Delay before really erasing the cookies of a deleted isolated space (time to undo). */
const SPACE_UNDO_MS = 30_000

export interface SpaceActionsDeps {
  store: Store
  window: { readonly win: BrowserWindow | null }
  model: Model
  changed: Changed
  layout: Pick<Layout, 'requestFocus'>
  overlay: Pick<Overlay, 'openPopover'>
  undo: Pick<Undo, 'announce' | 'expire'>
  views: Pick<Views, 'viewOf'>
}

/** Spaces: switch, create, configure, delete (undoable), order, isolated profile, link routing rules. */
export class SpaceActions {
  constructor(private readonly d: SpaceActionsDeps) {}

  /**
   * Immediate hint for renders that did not make an optimistic choice (overlay popover):
   * goes out before the deferred apply → layout → state cycle. No effect if the space does not exist.
   */
  hint(spaceId: string, from?: WebContents): void {
    if (!this.d.model.spaces.has(spaceId)) return
    const win = this.d.window.win
    const wc = win && !win.isDestroyed() ? win.webContents : null
    if (wc && wc !== from) pushUi(wc, 'space:pending', spaceId)
  }

  switchTo(spaceId: string): void {
    if (!this.d.model.spaces.switchTo(spaceId)) return
    this.d.layout.requestFocus()
    this.d.changed()
  }

  create(draft: SpaceDraft): void {
    const space = this.d.model.spaces.create(draft)
    this.d.store.scheduleBookmarks()
    this.switchTo(space.id)
  }

  update(spaceId: string, draft: SpaceDraft): void {
    if (this.d.model.spaces.update(spaceId, draft)) this.d.changed()
  }

  /** Changes a space's profile after confirmation; its tabs reload in the new session. */
  async setProfile(spaceId: string, profile: 'shared' | 'isolated'): Promise<void> {
    const { store, window, model } = this.d
    const space = store.state.spaces.find((s) => s.id === spaceId)
    if (!space || (space.profile ?? 'shared') === profile) return
    if (model.spaces.tabIds(space).length > 0) {
      const options = {
        type: 'question' as const,
        message: t(profile === 'isolated' ? 'dialog.isolateProfile.message' : 'dialog.shareProfile.message', { name: space.name }),
        detail: t(profile === 'isolated' ? 'dialog.isolateProfile.detail' : 'dialog.shareProfile.detail'),
        buttons: [t(profile === 'isolated' ? 'dialog.isolateProfile.confirm' : 'dialog.shareProfile.confirm'), t('dialog.cancel')],
        defaultId: 0,
        cancelId: 1
      }
      const { response } = window.win ? await dialog.showMessageBox(window.win, options) : await dialog.showMessageBox(options)
      if (response !== 0) return
    }
    // The dialog is asynchronous: the space may have disappeared or changed tabs in the meantime.
    if (model.spaces.setProfile(space, profile)) this.d.changed()
  }

  addRoute(pattern: string, spaceId: string): void {
    if (this.d.model.spaces.addRoute(pattern, spaceId)) this.d.changed()
  }

  removeRoute(routeId: string): void {
    this.d.model.spaces.removeRoute(routeId)
    this.d.changed()
  }

  /** Clears the data of an isolated space's partition, after confirmation. */
  async clearData(spaceId: string): Promise<void> {
    const { store, window, model, views } = this.d
    const space = store.state.spaces.find((s) => s.id === spaceId)
    if (!space || !isIsolated(space)) return
    const options = {
      type: 'warning' as const,
      message: t('dialog.clearSpaceData.message', { name: space.name }),
      detail: t('dialog.clearSpaceData.detail'),
      buttons: [t('dialog.clearSpaceData.confirm'), t('dialog.cancel')],
      defaultId: 1,
      cancelId: 1
    }
    const { response } = window.win ? await dialog.showMessageBox(window.win, options) : await dialog.showMessageBox(options)
    if (response !== 0) return
    await clearSpaceData(spaceId)
    for (const id of model.spaces.tabIds(space)) views.viewOf(id)?.webContents.reload()
  }

  /** Shifts a space by one rank (left/right); the active space stays the same. */
  move(spaceId: string, step: -1 | 1): void {
    if (this.d.model.spaces.move(spaceId, step)) this.d.changed()
  }

  /**
   * Deletes a space and its tabs, with a notice to undo. Isolated space: its cookies are only erased once
   * the undo window has passed (or the notice is replaced).
   */
  delete(spaceId: string): void {
    const { store, model, layout, undo } = this.d
    const removed = model.spaces.delete(spaceId)
    if (!removed) return
    const { space } = removed
    layout.requestFocus()
    store.scheduleBookmarks()
    this.d.changed()
    let settled = false
    let timer: NodeJS.Timeout | null = null
    const commit = () => {
      if (settled) return
      settled = true
      if (timer) clearTimeout(timer)
      if (isIsolated(space))
        void clearSpaceData(space.id).then(
          () => clearPendingWipe(space.id),
          () => {}
        )
    }
    if (isIsolated(space)) markPendingWipe(space.id) // the app may quit before the undo finishes
    timer = setTimeout(() => {
      commit()
      undo.expire(commit)
    }, SPACE_UNDO_MS)
    undo.announce(
      'spaceDeleted',
      removed.tabCount,
      space.name,
      () => {
        if (settled) return
        settled = true
        if (timer) clearTimeout(timer)
        clearPendingWipe(space.id)
        if (!removed.restore()) return
        layout.requestFocus()
        store.scheduleBookmarks()
        this.d.changed()
      },
      commit
    )
  }

  /** Opens a space's form (`null`: new space) in the visible UI. */
  edit(spaceId: string | null): void {
    // Tab row: the form opens under the space switcher, above the pages.
    if (isHorizontal(this.d.store.state)) return this.d.overlay.openPopover({ kind: 'space-form', spaceId })
    if (this.d.window.win) pushUi(this.d.window.win.webContents, 'space:edit', { spaceId })
  }

  cycle(step: number): void {
    this.switchTo(this.d.model.spaces.cycleTarget(step))
  }

  /** Neighboring space without wrapping (three-finger gesture). */
  step(step: number): void {
    const next = this.d.model.spaces.stepTarget(step)
    if (next) this.switchTo(next)
  }

  goto(n: number): void {
    const target = this.d.model.spaces.numberTarget(n)
    if (target) this.switchTo(target)
  }
}

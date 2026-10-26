import type { BrowserWindow } from 'electron'
import { pushUi } from '../ipc/push'
import type { Store } from '../core/store'
import type { Model } from '../model'
import { isHorizontal, isSidebarOn } from '../model/chrome'
import type { Folder } from '@signet/shared/tree'
import type { Changed } from './broadcast'
import { REVEAL_MS, type Floating } from './floating'
import type { Overlay } from './overlay'
import type { Undo } from './undo'

export interface FolderActionsDeps {
  store: Store
  window: { readonly win: BrowserWindow | null }
  model: Model
  changed: Changed
  floating: Pick<Floating, 'shown' | 'target'>
  overlay: Pick<Overlay, 'openPopover'>
  undo: Pick<Undo, 'announce'>
}

/** Pinned folders: creation, settings, deletion, filing a tab, renaming in the UI. */
export class FolderActions {
  constructor(private readonly d: FolderActionsDeps) {}

  /** Puts the name of a folder, or of a tab group (same channel), into edit mode in the visible UI. */
  requestRename(folderId: string): void {
    const { store, window, model } = this.d
    const state = store.state
    if (isHorizontal(state)) {
      // Tab row: the row places the field under the badge; in compact mode it is hidden, so we open directly.
      const target = model.groups.header(folderId) ? ({ kind: 'group', groupId: folderId } as const) : ({ kind: 'folder', folderId } as const)
      if (state.settings.compactMode) this.d.overlay.openPopover({ kind: 'rename', target })
      else if (window.win) pushUi(window.win.webContents, 'folder:rename', { folderId })
      return
    }
    if (!isSidebarOn(state) && !this.d.floating.shown) {
      state.sidebarVisible = true
      state.settings.compactMode = false
      this.d.changed()
      // The bar only exists on screen after this cycle: we talk to it once rendered.
      setTimeout(() => {
        if (window.win && !window.win.isDestroyed()) pushUi(window.win.webContents, 'folder:rename', { folderId })
      }, REVEAL_MS)
      return
    }
    const target = this.d.floating.target
    if (target) target.send('folder:rename', { folderId })
    else if (window.win) pushUi(window.win.webContents, 'folder:rename', { folderId })
  }

  /** Creates a folder at the top of the active space's pinned items (or of `parentId`) and starts its renaming. */
  create(parentId?: string): Folder {
    const folder = this.d.model.folders.create(parentId)
    this.d.changed()
    this.requestRename(folder.id)
    return folder
  }

  rename(folderId: string, name: string): void {
    if (this.d.model.folders.rename(folderId, name)) this.d.changed()
  }

  /** Hue of a folder's card; `null` removes it. */
  setHue(folderId: string, hue: number | null): void {
    if (this.d.model.folders.setHue(folderId, hue)) this.d.changed()
  }

  /** Emoji in place of the folder icon; absent = folder icon. */
  setIcon(folderId: string, icon: string | undefined): void {
    if (this.d.model.folders.setIcon(folderId, icon)) this.d.changed()
  }

  toggle(folderId: string, open?: boolean): void {
    if (this.d.model.folders.toggle(folderId, open)) this.d.changed()
  }

  /** Deletes the folder; its items move up into its place, or its tabs are destroyed (`withTabs`) with a notice to undo. */
  delete(folderId: string, withTabs: boolean): void {
    const result = this.d.model.folders.delete(folderId, withTabs)
    if (!result) return
    this.d.changed()
    const { removed } = result
    if (removed) {
      this.d.undo.announce('tabsClosed', removed.count, '', () => {
        if (removed.restore()) this.d.changed()
      })
    }
  }

  /** Files the tab (or its split view) at the end of a folder of its space; `null` = new folder. */
  addTo(tabId: string, folderId: string | null): void {
    const result = this.d.model.folders.addTo(tabId, folderId)
    if (!result) return
    if (result.moved) this.d.changed()
    if (result.created) this.requestRename(result.folder.id)
  }
}

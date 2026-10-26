import type { DragSource, DropTarget } from '@shared/types'
import type { Model } from '../model'
import type { Changed } from './broadcast'

export interface ArrangeDeps {
  model: Model
  changed: Changed
}

/** Moves in the sidebar: drag and drop, pin, favorite, move up or down, change space. */
export class Arrange {
  constructor(private readonly d: ArrangeDeps) {}

  moveItem(source: DragSource, target: DropTarget): void {
    if (this.d.model.lists.moveItem(source, target)) this.d.changed()
  }

  /**
   * Pins a group: it becomes a pinned folder (same name, emoji, hue, collapsed state), at the end of the pinned items
   * of its space, or at the place targeted by a drag and drop (root or inside a folder).
   */
  pinGroup(groupId: string, target?: DropTarget): void {
    if (this.d.model.lists.pinGroup(groupId, target)) this.d.changed()
  }

  /** Unpins a folder: its tabs become a group of today's tabs. */
  unpinFolder(folderId: string, target?: DropTarget): void {
    if (this.d.model.lists.unpinFolder(folderId, target)) this.d.changed()
  }

  togglePin(tabId: string): void {
    if (this.d.model.lists.togglePin(tabId)) this.d.changed()
  }

  toggleFavorite(tabId: string): void {
    if (this.d.model.lists.toggleFavorite(tabId)) this.d.changed()
  }

  /** Moves a tab up (-1) or down (+1) one rank in its list (alternative to drag and drop). */
  moveTabStep(tabId: string, dir: -1 | 1): void {
    if (this.d.model.lists.moveTabStep(tabId, dir)) this.d.changed()
  }

  moveTabToSpace(tabId: string, spaceId: string): void {
    if (this.d.model.lists.moveTabToSpace(tabId, spaceId)) this.d.changed()
  }
}

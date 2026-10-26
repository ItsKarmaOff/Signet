import type { Model } from '../model'
import { spaceOfGroup } from '../model/locate'
import type { Space } from '@shared/types'
import type { Store } from '../core/store'
import type { Changed } from './broadcast'
import type { Undo } from './undo'

export interface GroupActionsDeps {
  store: Store
  model: Model
  changed: Changed
  undo: Pick<Undo, 'announce'>
}

/** Today's tab groups: closing as a block, taking a tab out, header (name, hue, emoji, collapse). */
export class GroupActions {
  constructor(private readonly d: GroupActionsDeps) {}

  closeById(groupId: string): void {
    const space = spaceOfGroup(this.d.store.state, groupId)
    if (space) this.close(space, groupId)
  }

  /** Closes all the group's tabs; ⇧⌘T reopens them together, with their header. */
  close(space: Space, groupId: string): void {
    const removed = this.d.model.groups.close(space, groupId)
    if (!removed) return
    this.d.changed()
    this.d.undo.announce('tabsClosed', removed.count, '', () => {
      if (removed.restore()) this.d.changed()
    })
  }

  /** Takes the tab (or its split view) out of its group: it is placed right after the group. */
  leave(tabId: string): void {
    if (this.d.model.groups.leave(tabId)) this.d.changed()
  }

  /** Dissolves the group: its tabs stay in place, with no link between them. */
  ungroup(groupId: string): void {
    this.d.model.groups.ungroup(groupId)
    this.d.changed()
  }

  rename(groupId: string, name: string): void {
    if (this.d.model.groups.rename(groupId, name)) this.d.changed()
  }

  setHue(groupId: string, hue: number): void {
    if (this.d.model.groups.setHue(groupId, hue)) this.d.changed()
  }

  /** Emoji in place of the first tab's icon; absent = first tab's icon. */
  setIcon(groupId: string, icon: string | undefined): void {
    if (this.d.model.groups.setIcon(groupId, icon)) this.d.changed()
  }

  toggle(groupId: string, open?: boolean): void {
    if (this.d.model.groups.toggle(groupId, open)) this.d.changed()
  }
}

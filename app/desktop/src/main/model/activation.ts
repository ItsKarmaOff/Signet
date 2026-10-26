// Active tab of each space: activation, and choosing the next one when the active one goes away.
import type { Item, Space } from '@shared/types'
import { itemTabIds } from '@signet/shared/tree'
import { locate, reachable, activeSpace, type Location } from './locate'
import type { ModelData, ModelEnv } from './types'

export class Activation {
  constructor(
    private readonly data: ModelData,
    private readonly env: ModelEnv
  ) {}

  setActive(space: Space, tabId: string | null): void {
    space.activeTabId = tabId
    if (!tabId) return
    const tab = this.data.state.tabs[tabId]
    if (tab) tab.lastActive = this.env.now()
    this.env.onActivated(space, tabId)
  }

  /**
   * Activates the tab (and the space that contains it). Returns the tab that was active before, or `undefined` if the tab is not found.
   */
  activate(tabId: string): { before: string | null } | undefined {
    const { state } = this.data
    const loc = locate(state, tabId)
    if (!loc) return undefined
    const before = activeSpace(state).activeTabId
    if (loc.space) state.activeSpaceId = loc.space.id
    this.setActive(activeSpace(state), tabId)
    return { before }
  }

  /** First usable tab after the removal of item `index` (or staying in place if `removed` is false). */
  pickNext(space: Space, list: Item[], index: number, removed: boolean, skip: Set<string>): string | null {
    const before = list.slice(0, index).reverse()
    const order = removed ? [...list.slice(index), ...before] : [list[index], ...list.slice(index + 1), ...before]
    for (const item of [...order, ...space.pinned]) {
      const id = item && itemTabIds(item).find((t) => !skip.has(t))
      if (id) return id
    }
    return null
  }

  /**
   * After closing (or removing) a favorite: the most recently used open tab (today tabs, pinned,
   * already loaded favorites), otherwise the first today or pinned tab. Never a sleeping favorite: it is not woken up.
   */
  afterFavorite(space: Space, skip: Set<string>): string | null {
    const { tabs, favorites } = this.data.state
    const open = [...space.today, ...space.pinned].flatMap(itemTabIds).filter((id) => !skip.has(id))
    const live = [...open, ...favorites.filter((id) => !skip.has(id))].filter((id) => tabs[id]?.loaded)
    const recent = live.sort((x, y) => (tabs[y].lastActive ?? 0) - (tabs[x].lastActive ?? 0))[0]
    return recent ?? open[0] ?? null
  }

  /** After a removal or an unload: picks a new active tab where the old one is no longer reachable. */
  repick(loc: Location, ids: string[], removed: boolean): void {
    const { state } = this.data
    const skip = new Set(ids)
    for (const space of state.spaces) {
      const current = space.activeTabId
      if (!current || !skip.has(current)) continue
      if (removed && reachable(state, space, current)) continue
      const here = loc.zone === 'favorites' || loc.space === space
      const next =
        loc.zone === 'favorites'
          ? this.afterFavorite(space, skip)
          : here
            ? this.pickNext(space, loc.list, loc.index, removed, skip)
            : this.pickNext(space, space.pinned, 0, true, skip)
      this.setActive(space, next)
    }
  }
}

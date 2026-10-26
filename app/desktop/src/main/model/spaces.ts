// Spaces: creation, settings, order, undoable deletion, link routing rules.
import type { Space, SpaceDraft } from '@shared/types'
import { itemTabIds } from '@signet/shared/tree'
import { cleanSpaceDraft, newSpace } from '@signet/shared/model'
import { normalizePattern } from '@signet/shared/routes'
import type { ClosedTabs } from './closed'
import { activeSpace, locate } from './locate'
import type { Removal } from './removal'
import type { ModelData, ModelEnv } from './types'

/** Deleted space: what is needed to announce it and put it back. */
export interface SpaceRemoved {
  space: Space
  /** Number of tabs destroyed along with the space. */
  tabCount: number
  /** Puts back the space, its tabs and its routing rules; false if it already came back. */
  restore: () => boolean
}

export class Spaces {
  constructor(
    private readonly data: ModelData,
    private readonly env: ModelEnv,
    private readonly closed: ClosedTabs,
    private readonly removal: Removal
  ) {}

  has(spaceId: string): boolean {
    return this.data.state.spaces.some((s) => s.id === spaceId)
  }

  /** Tabs of a space: pinned then day tabs. */
  tabIds(space: Space): string[] {
    return [...space.pinned, ...space.today].flatMap(itemTabIds)
  }

  /** Makes this space the active space. False if it does not exist. */
  switchTo(spaceId: string): boolean {
    if (!this.has(spaceId)) return false
    this.data.state.activeSpaceId = spaceId
    return true
  }

  create(draft: SpaceDraft): Space {
    const space = newSpace(cleanSpaceDraft(draft, this.env.defaultSpaceName()))
    this.data.state.spaces.push(space)
    return space
  }

  update(spaceId: string, draft: SpaceDraft): boolean {
    const space = this.data.state.spaces.find((s) => s.id === spaceId)
    if (!space) return false
    const clean = cleanSpaceDraft(draft, this.env.defaultSpaceName())
    Object.assign(space, clean)
    if (!clean.theme) delete space.theme
    return true
  }

  /** Changes a space's profile (the space may have disappeared during the confirmation); its tabs reload in the new session. */
  setProfile(space: Space, profile: 'shared' | 'isolated'): boolean {
    if (!this.data.state.spaces.includes(space)) return false
    space.profile = profile
    this.env.reconcileSessions(this.tabIds(space))
    return true
  }

  addRoute(pattern: string, spaceId: string): boolean {
    const clean = normalizePattern(pattern)
    if (!clean || !this.has(spaceId)) return false
    const { routes } = this.data.state.settings
    const same = routes.find((r) => r.pattern === clean)
    if (same) same.spaceId = spaceId
    else routes.push({ id: this.env.newId(), pattern: clean, spaceId })
    return true
  }

  removeRoute(routeId: string): void {
    const { settings } = this.data.state
    settings.routes = settings.routes.filter((r) => r.id !== routeId)
  }

  /** Shifts a space by one rank (left/right); the active space stays the same. */
  move(spaceId: string, step: -1 | 1): boolean {
    const { spaces } = this.data.state
    const from = spaces.findIndex((s) => s.id === spaceId)
    const to = from + step
    if (from < 0 || to < 0 || to >= spaces.length) return false
    const [space] = spaces.splice(from, 1)
    spaces.splice(to, 0, space)
    return true
  }

  /** Next or previous space, with wrapping. */
  cycleTarget(step: number): string {
    const { spaces } = this.data.state
    const i = spaces.findIndex((s) => s.id === activeSpace(this.data.state).id)
    return spaces[(i + step + spaces.length) % spaces.length].id
  }

  /** Neighboring space without wrapping (three-finger gesture). */
  stepTarget(step: number): string | undefined {
    const { spaces } = this.data.state
    return spaces[spaces.findIndex((s) => s.id === activeSpace(this.data.state).id) + step]?.id
  }

  /** Nth space (starting from 1). */
  numberTarget(n: number): string | undefined {
    return this.data.state.spaces[n - 1]?.id
  }

  /**
   * Deletes a space with its tabs (which join the ⇧⌘T stack as one batch). Nothing if it is the last space or it does not exist.
   * If the deleted space was the active one, its neighbor becomes it.
   */
  delete(spaceId: string): SpaceRemoved | null {
    const { state } = this.data
    if (state.spaces.length < 2) return null
    const index = state.spaces.findIndex((s) => s.id === spaceId)
    if (index < 0) return null
    // The tabs join the ⇧⌘T stack while the space still exists (to know their position).
    const tabIds = this.tabIds(state.spaces[index])
    const batch = this.closed.nextBatch()
    for (const id of [...tabIds].reverse()) this.closed.remember(id, locate(state, id), batch)
    const snap = this.closed.snapshotTabs(tabIds)
    const routes = state.settings.routes.filter((r) => r.spaceId === spaceId)
    const wasActive = state.activeSpaceId === spaceId
    const [space] = state.spaces.splice(index, 1)
    state.settings.routes = state.settings.routes.filter((r) => r.spaceId !== spaceId)
    this.removal.drop(tabIds)
    if (state.activeSpaceId === spaceId) state.activeSpaceId = state.spaces[Math.min(index, state.spaces.length - 1)].id
    const restore = (): boolean => {
      const s = this.data.state
      if (s.spaces.some((sp) => sp.id === space.id)) return false
      this.closed.restoreTabs(snap)
      this.closed.dropBatch(batch)
      s.spaces.splice(Math.min(index, s.spaces.length), 0, space)
      for (const r of routes) if (!s.settings.routes.some((x) => x.id === r.id)) s.settings.routes.push(r)
      if (wasActive) s.activeSpaceId = space.id
      return true
    }
    return { space, tabCount: tabIds.length, restore }
  }
}

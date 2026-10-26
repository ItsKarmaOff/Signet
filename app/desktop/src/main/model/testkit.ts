// Model test tooling: a minimal state, spied effects and a compact way to write lists.
import type { Item, Settings, Space, TabInfo } from '@shared/types'
import type { State } from '../core/store'
import { newTab } from '@signet/shared/model'
import { Model } from './index'
import type { ModelData, ModelEnv } from './types'

export const tab = (tabId: string, group?: string): Item => ({ kind: 'tab', tabId, ...(group ? { group } : {}) })
export const folder = (id: string, items: Item[], extra: Partial<Extract<Item, { kind: 'folder' }>> = {}): Item => ({
  kind: 'folder',
  id,
  name: id,
  open: true,
  items,
  ...extra
})
export const split = (id: string, tabIds: string[], group?: string): Item => ({ kind: 'split', id, tabIds, ...(group ? { group } : {}) })

/** Compact list notation: `a`, `a^g` (group g), `[f: a b]` (folder), `{s: a b}` (split view). */
export function shape(list: Item[]): string[] {
  return list.map((item) => {
    if (item.kind === 'tab') return item.group ? `${item.tabId}^${item.group}` : item.tabId
    if (item.kind === 'split') return `{${item.id}: ${item.tabIds.join(' ')}}${item.group ? `^${item.group}` : ''}`
    return `[${item.id}: ${shape(item.items).join(' ')}]`
  })
}

export interface WorldSpec {
  /** Tabs to create (their properties by id). */
  tabs?: string[] | Record<string, Partial<TabInfo>>
  favorites?: string[]
  spaces?: Partial<Space>[]
  active?: string
  settings?: Partial<Settings>
}

/** Effects requested by the model, in order. */
export interface Effects {
  destroyed: string[]
  saved: string[]
  reconciled: string[][]
  activated: [string, string][]
}

export function world(spec: WorldSpec = {}) {
  const tabsSpec = Array.isArray(spec.tabs) ? Object.fromEntries(spec.tabs.map((id) => [id, {}])) : (spec.tabs ?? {})
  const tabs: Record<string, TabInfo> = {}
  for (const [id, props] of Object.entries(tabsSpec)) tabs[id] = { ...newTab(`https://${id}.test/`, id), id, lastActive: 0, ...props }
  const spaces: Space[] = (spec.spaces ?? [{}]).map((s, i) => ({
    id: `s${i + 1}`,
    name: `Espace ${i + 1}`,
    icon: 'x',
    hue: 100,
    pinned: [],
    today: [],
    activeTabId: null,
    ...s
  }))
  const state = {
    tabs,
    favorites: spec.favorites ?? [],
    spaces,
    activeSpaceId: spec.active ?? spaces[0].id,
    archive: [],
    settings: { routes: [], archiveAfterHours: 12, ...spec.settings }
  } as unknown as State
  const data: ModelData = { state, histories: new Map(), closed: [], detached: new Set() }
  const effects: Effects = { destroyed: [], saved: [], reconciled: [], activated: [] }
  const clock = { now: 1_000_000 }
  let seq = 0
  const env: ModelEnv = {
    newId: () => `id${++seq}`,
    now: () => clock.now,
    defaultFolderName: () => 'Folder',
    defaultSpaceName: () => 'Espace',
    onActivated: (space, tabId) => effects.activated.push([space.id, tabId]),
    destroyView: (tabId) => {
      effects.destroyed.push(tabId)
      if (state.tabs[tabId]) Object.assign(state.tabs[tabId], { loaded: false })
    },
    saveHistory: (tabId) => effects.saved.push(tabId),
    reconcileSessions: (ids) => effects.reconciled.push([...ids])
  }
  const model = new Model(data, env)
  return { model, data, state, effects, clock, space: (id = 's1') => spaces.find((s) => s.id === id)! }
}

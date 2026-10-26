// Intermediate structure produced by the importers (pure functions, no disk or Electron access).

export interface ImportedTab {
  url: string
  title: string
}

export type ImportedNode = ({ kind: 'tab' } & ImportedTab) | { kind: 'folder'; name: string; items: ImportedNode[] } | { kind: 'split'; tabs: ImportedTab[] }

export interface ImportedSpace {
  name: string
  icon: string
  pinned: ImportedNode[]
  today: ImportedNode[]
}

export interface SpacesImport {
  spaces: ImportedSpace[]
  favorites: ImportedTab[]
}

export const isWebUrl = (u: unknown): u is string => typeof u === 'string' && /^https?:\/\//i.test(u.trim())

/** Recursively removes empty folders. */
export function pruneEmpty(nodes: ImportedNode[]): ImportedNode[] {
  return nodes.flatMap((n): ImportedNode[] => {
    if (n.kind !== 'folder') return [n]
    const items = pruneEmpty(n.items)
    return items.length ? [{ ...n, items }] : []
  })
}

export function countTabs(nodes: ImportedNode[]): number {
  return nodes.reduce((n, x) => n + (x.kind === 'tab' ? 1 : x.kind === 'split' ? x.tabs.length : countTabs(x.items)), 0)
}

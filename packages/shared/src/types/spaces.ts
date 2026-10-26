import type { Item, TabGroup } from './tabs'

/** Source of a live folder. */
export type LiveSource = { kind: 'feed'; url: string } | { kind: 'github'; query: GithubQuery }

/** GitHub queries offered: review requests addressed to me, or my open pull requests. */
export type GithubQuery = 'review-requested' | 'mine'

/** State of a live folder (inside the folder itself). */
export interface LiveFolder {
  source: LiveSource
  /** Last successful update (ms). */
  refreshedAt?: number
  /** Last failure, translated message; absent if the last update succeeded. */
  error?: string
  /** Update in progress (never read back at startup). */
  refreshing?: boolean
  /** Addresses already opened: the others carry a "new" dot. */
  seen?: string[]
}

/** Routing rule: new tabs whose host matches open in `spaceId`. */
export interface SpaceRoute {
  id: string
  /** Domain (`github.com` also covers its subdomains) or pattern with `*` (`*.notion.site`). */
  pattern: string
  spaceId: string
}

export interface Space {
  id: string
  name: string
  /** Un emoji. */
  icon: string
  /** HSL hue 0–359 that colors the sidebar; with a gradient, it is its first color (accent). */
  hue: number
  /** Background gradient of the space. Missing = solid color (`hue`). */
  theme?: SpaceTheme
  pinned: Item[]
  today: Item[]
  /** Tab that has focus in this space (can be a favorite or a tab of a split view). */
  activeTabId: string | null
  /** `isolated`: cookies, storage and accounts specific to the space. Missing = `shared`. */
  profile?: 'shared' | 'isolated'
  /** Day tab groups, by id (see `Item.group`). */
  groups?: Record<string, TabGroup>
  /** Space bookmarks (bar below the address bar), in order; folders can nest. */
  bookmarks?: Bookmark[]
}

/** Bookmark: a link, or a folder of bookmarks. */
export type Bookmark =
  { kind: 'link'; id: string; title: string; url: string; favicon?: string } | { kind: 'folder'; id: string; name: string; items: Bookmark[] }

/** Where to drop a bookmark in the active space: at the root of the bar or in a folder, at position `index`. */
export interface BookmarkTarget {
  folderId?: string
  index: number
}

export interface SpaceDraft {
  name: string
  icon: string
  hue: number
  /** Gradient; `null` or missing = solid color. */
  theme?: SpaceTheme | null
}

/** Background of a gradient space: 2 or 3 hues (the first is also `Space.hue`) and an optional grain. */
export interface SpaceTheme {
  /** HSL hues 0–359, in gradient order (2 or 3). */
  hues: number[]
  /** Texture grain, from 0 (smooth) to 1. */
  grain: number
}

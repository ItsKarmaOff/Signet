import type { SiteInfo } from './permissions'
import type { Rect } from '@signet/shared/types'
import type { IconTarget } from './tabs'

export type PaletteMode = 'new-tab' | 'edit-url' | 'split'

export type PaletteResult =
  | { kind: 'url'; id: string; title: string; subtitle?: string; url: string }
  | { kind: 'search'; id: string; title: string; subtitle?: string; url: string }
  | { kind: 'suggestion'; id: string; title: string; subtitle?: string; url: string }
  | { kind: 'history'; id: string; title: string; subtitle?: string; url: string; favicon?: string; inline?: boolean }
  | { kind: 'archived'; id: string; title: string; subtitle?: string; url: string; favicon?: string; inline?: boolean }
  /** `inline`: set by main on the top row only, when the page was typed or visited enough to complete the input. */
  | { kind: 'tab'; id: string; title: string; subtitle?: string; tabId: string; url?: string; favicon?: string; inline?: boolean }
  | {
      kind: 'action'
      id: string
      title: string
      subtitle?: string
      action: string
      shortcut?: string
      /** Preview (data URL) shown in place of the icon, e.g. for app icon variants. */
      image?: string
    }

/** What the main window's overlay must show. Pushed on `overlay:show`. */
export type OverlayPayload =
  | { mode: 'palette'; paletteMode: PaletteMode; initial: string; hue: number; anchor?: Rect }
  | { mode: 'peek'; tabId: string; url: string; title: string; card: Rect; origin?: { x: number; y: number } }
  /** Site panel, anchored under the toolbar's padlock. */
  | { mode: 'site'; info: SiteInfo; anchor: Rect }
  /** Drop zones over the page while a tab is dragged from the sidebar. */
  | {
      mode: 'dropzones'
      area: Rect
      /** Card of the focused pane if a split view can be added to it (fewer than 4 panes, not a favorite), otherwise null. */ splitCard: Rect | null
    }
  /** Dropdown menu or form of the tab row, anchored under `anchor` and placed above the pages. */
  | { mode: 'popover'; popover: PopoverRequest; anchor: Rect; hue: number }
  /** Prompt to save (or update) a password, at the top right of `card`. The password stays in the main process. */
  | {
      mode: 'password'
      token: string
      origin: string
      username: string
      update: boolean
      card: Rect
      /** Opened from the key button: card under it. */
      anchor?: Rect
    }
  /** Saved accounts of the site (without passwords), under the key button of the address bar. */
  | { mode: 'passwords'; origin: string; logins: { id: string; username: string }[]; anchor: Rect }
  /** Screenshot tool placed over the tab's web view (`rect`, window coordinates). */
  | { mode: 'capture'; tabId: string; rect: Rect; hue: number }
  /**
   * Install the site as an app, at the top right of `card`. `icon`: best icon found (data URL), or null;
   * `maskable`: it can fill the whole icon square (otherwise it is placed on a white background).
   */
  | {
      mode: 'webapp'
      token: string
      name: string
      url: string
      icon: string | null
      maskable: boolean
      hue: number
      card: Rect
    }

/** What a tab-row popover shows (`horizontal` layout). */
export type PopoverRequest =
  | { kind: 'spaces' }
  | { kind: 'space-form'; spaceId: string | null }
  /** Live folder form: creation (`folderId` null) or source change. */
  | { kind: 'live-folder'; folderId: string | null }
  | { kind: 'folder'; folderId: string }
  | { kind: 'downloads' }
  | { kind: 'icon'; target: IconTarget }
  | {
      kind: 'rename'
      target: { kind: 'tab'; tabId: string } | { kind: 'folder'; folderId: string } | { kind: 'group'; groupId: string } | { kind: 'bookmark'; id: string }
    }
  /** Contents of a bookmarks folder (or, with `folderId` null, of the bar starting at `from`: what overflows). `tabId`: the pane's tab. */
  | { kind: 'bookmarks'; folderId: string | null; from?: number; tabId: string | null }

/** "Hold ⌘Q to quit" bubble. Pushed on `quit:state` to the `#quit` view. */
export interface QuitHudState {
  /** Progress at rest (0); while pressed, the gauge is computed in the view from `start` and `duration`. */
  progress: number
  /** True while pressed (the gauge fills); false once released (waiting for a second press). */
  holding: boolean
  /** Press start (ms, Date.now() of main) and hold duration: sent only once. */
  start?: number
  duration?: number
  /** Expected delay of the first key repeat (ms): past it, the gauge slows down until the repeat arrives. */
  wait?: number
  /** True as soon as key repeat proves the key is held: the gauge fills up to the end. */
  repeating?: boolean
}

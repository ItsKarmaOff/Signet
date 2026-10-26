// Data and effects the model needs. No Electron import: effects (views, sessions…) are injected.
import type { Space } from '@shared/types'
import type { ClosedTab, State, TabHistory } from '../core/store'

/** What the model reads and changes: the persisted state and the structures that go with it (`Store` fits as is). */
export interface ModelData {
  readonly state: State
  /** Historique de navigation par onglet. */
  readonly histories: Map<string, TabHistory>
  /** Stack of recently closed tabs (⇧⌘T). */
  readonly closed: ClosedTab[]
  /** Tabs with no place in the sidebar (Peek, Little Signet). */
  readonly detached: Set<string>
}

export interface ModelEnv {
  newId(): string
  now(): number
  /** (Translated) name of a folder or group created without a name. */
  defaultFolderName(): string
  /** (Translated) name of a space created without a name. */
  defaultSpaceName(): string
  /** A tab becomes the active one of a space (live folders, focus…). */
  onActivated(space: Space, tabId: string): void
  /** Destroys a tab's web view (the tab stays in the state, unloaded). */
  destroyView(tabId: string): void
  /** Copies the view's navigation history into `histories`. */
  saveHistory(tabId: string): void
  /** Destroys the views that are no longer in the right session after a move. */
  reconcileSessions(tabIds: string[]): void
}

export const clamp = (n: number, min: number, max: number): number => Math.min(max, Math.max(min, n))

/** Number of tabs kept in the archive. */
export const ARCHIVE_MAX = 500

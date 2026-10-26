// Persisted-state model: the rules for organizing tabs (open, close, move, archive, group…), without Electron.
import type { Space, TabInfo } from '@shared/types'
import { Activation } from './activation'
import { Archive } from './archive'
import { ClosedTabs } from './closed'
import { Folders } from './folders'
import { Groups } from './groups'
import { Lists } from './lists'
import { activeSpace, activeTab, displayedTabIds, findFolder, findSplit, locate, type Location } from './locate'
import { cycleTarget, numberTarget } from './navigation'
import { Opening } from './opening'
import { Removal } from './removal'
import { Spaces } from './spaces'
import { Splits } from './splits'
import type { ModelData, ModelEnv } from './types'

export type { Location } from './locate'
export type { ModelData, ModelEnv } from './types'

export class Model {
  readonly activation: Activation
  readonly opening: Opening
  readonly closed: ClosedTabs
  readonly lists: Lists
  readonly splits: Splits
  readonly removal: Removal
  readonly archive: Archive
  readonly folders: Folders
  readonly groups: Groups
  readonly spaces: Spaces

  constructor(
    private readonly data: ModelData,
    env: ModelEnv
  ) {
    this.activation = new Activation(data, env)
    this.opening = new Opening(data, env, this.activation)
    this.closed = new ClosedTabs(data, env, this.activation, this.opening)
    this.lists = new Lists(data, env, this.activation)
    this.splits = new Splits(data, env, this.activation, this.lists)
    this.removal = new Removal(data, env, this.activation, this.closed, this.lists, this.splits)
    this.archive = new Archive(data, env, this.activation, this.removal)
    this.folders = new Folders(data, env, this.activation, this.closed, this.lists, this.removal)
    this.groups = new Groups(data, this.activation, this.closed, this.removal)
    this.spaces = new Spaces(data, env, this.closed, this.removal)
  }

  // Reads

  /** Espace actif. */
  get space(): Space {
    return activeSpace(this.data.state)
  }

  activeTab(): TabInfo | undefined {
    return activeTab(this.data.state)
  }

  /** Where the tab is in the sidebar. */
  locate(tabId: string): Location | null {
    return locate(this.data.state, tabId)
  }

  findFolder(folderId: string) {
    return findFolder(this.data.state, folderId)
  }

  findSplit(splitId: string) {
    return findSplit(this.data.state, splitId)
  }

  /** Tabs shown in the web card of the active space. */
  displayedTabIds(): string[] {
    return displayedTabIds(this.data.state, this.space)
  }

  /** Next (+1) or previous (-1) tab in the bar's order. */
  cycleTarget(step: number): string | undefined {
    return cycleTarget(this.data.state, step)
  }

  /** Tab number `n` (⌘1 to ⌘9). */
  numberTarget(n: number, horizontal: boolean): string | undefined {
    return numberTarget(this.data.state, n, horizontal)
  }
}

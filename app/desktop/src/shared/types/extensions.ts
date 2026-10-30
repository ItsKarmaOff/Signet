/** Installed extension. */
export interface ExtensionInfo {
  id: string
  name: string
  version: string
  /** Icon as a data URL, if the extension declares one. */
  icon?: string
  /** True if the extension has a toolbar button. */
  hasAction: boolean
  /** Keyboard commands from the manifest, with the shortcut each one really has (empty: none, or lost to a clash). */
  commands?: { name: string; description: string; shortcut: string }[]
  /** Unpacked extension, loaded from a folder (empty id if it could not be loaded). */
  unpacked?: { path: string; display: string; error?: string; reloadedAt?: number }
}

/** Blocked-request counts shown on extension buttons: tab (webContents id), then extension id, then count (always above 0). */
export type ExtensionActionCounts = Record<number, Record<string, number>>

/** Toolbar button state set by `action.enable/disable/setBadgeTextColor`: global values and per-tab ones (tab = webContents id). Only what differs from the defaults. */
export interface ExtensionActionState {
  enabled?: boolean
  textColor?: unknown
}
export type ExtensionActionStates = Record<string, ExtensionActionState & { tabs?: Record<number, ExtensionActionState> }>

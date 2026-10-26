/** Installed extension. */
export interface ExtensionInfo {
  id: string
  name: string
  version: string
  /** Icon as a data URL, if the extension declares one. */
  icon?: string
  /** True if the extension has a toolbar button. */
  hasAction: boolean
  /** Unpacked extension, loaded from a folder (empty id if it could not be loaded). */
  unpacked?: { path: string; display: string; error?: string; reloadedAt?: number }
}

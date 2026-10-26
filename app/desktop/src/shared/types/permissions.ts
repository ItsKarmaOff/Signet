export type PermissionState = 'ask' | 'allow' | 'deny'

export interface SitePermissionEntry {
  origin: string
  permission: string
  decision: 'allow' | 'deny'
}

/** What the site panel (click on the padlock) shows. */
export interface SiteInfo {
  tabId: string
  origin: string
  /** Registrable domain, like `siteOf()`. */
  site: string
  secure: boolean
  permissions: { id: string; label: string; state: PermissionState }[]
  boost: { exists: boolean; enabled: boolean }
  /** Requests blocked by the ad blocker since this tab last loaded. */
  blocked: number
  favicon?: string
}

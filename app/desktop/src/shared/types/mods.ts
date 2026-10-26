/** Customization of a site (CSS and hidden elements), applied to all its pages. */
export interface Boost {
  /** Registrable domain, as returned by `siteOf()` (e.g. `youtube.com`). */
  site: string
  enabled: boolean
  css: string
  /** CSS selectors of elements hidden with the "Hide" tool. */
  zapped: string[]
  updatedAt: number
}

/** State of a Boost's editing window. Pushed on `boost:state` to the `#boost` view. */
export interface BoostEditorState {
  site: string
  /** Existing Boost, or an empty draft if the site has none yet. */
  boost: Boost
  /** True while the "Hide" tool is waiting for a click in the page. */
  zapping: boolean
  /** False if no tab of this site is open any more (the "Hide" tool is then unavailable). */
  hasTab: boolean
  hue: number
}

/** UI mod: a CSS sheet applied to all the app's views (never to web pages). */
export interface InterfaceMod {
  id: string
  name: string
  css: string
  enabled: boolean
}

/** State of the Settings Mods section (`mods:list`). */
export interface ModsInfo {
  mods: InterfaceMod[]
  /** All mods turned off (setting, View menu). */
  disabled: boolean
  /** Launched in safe mode (`--safe-mode`): no mod is applied during this session. */
  safeMode: boolean
}

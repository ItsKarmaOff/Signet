/** State of the floating mini-player (custom picture-in-picture). Pushed on `pip:state` to the `#pip` view. */
export interface PipState {
  tabId: string
  site: string
  title: string
  playing: boolean
  muted: boolean
  /** Secondes. */
  currentTime: number
  /** Seconds; 0 or Infinity for a live stream. */
  duration: number
  hue: number
}

export type PipCommand = 'toggle' | 'back' | 'forward' | 'seek' | 'mute' | 'return' | 'close'

/** Tab of a private window, as its bar shows it. */
export interface PrivateTab {
  id: string
  url: string
  title: string
  favicon?: string
  loading: boolean
  audible: boolean
}

/** State of a private window's bar. Pushed on `private:state`. */
export interface PrivateState {
  tabs: PrivateTab[]
  activeId: string | null
  /** Sidebar width (same layout as the main window), null for tabs at the top. */
  sidebarWidth: number | null
  /** Address of the displayed tab ('' for a new empty tab). */
  url: string
  loading: boolean
  canGoBack: boolean
  canGoForward: boolean
}

/** State of a Little Signet window's bar. Pushed on `little:state`. */
export interface LittleState {
  tabId: string
  url: string
  title: string
  favicon?: string
  /** Color of the top of the page, to blend the bar into the site. */
  themeColor?: string
  loading: boolean
  canGoBack: boolean
  canGoForward: boolean
  hue: number
}

/** Site installed as an app (Settings › Apps). */
export interface WebAppInfo {
  id: string
  name: string
  startUrl: string
  /** 64 px icon (PNG as a data URL). */
  icon: string
  /** True if its launcher is in the apps folder. */
  launcher: boolean
  /** Space whose cookies and logins the app shares (isolated profile); null = common profile. */
  space: { name: string; icon: string } | null
  /** Number of open windows. */
  windows: number
}

export interface WebAppsInfo {
  apps: WebAppInfo[]
  /** Launchers folder (`~/Applications/Signet Apps`), for display. */
  folder: string
}

/** State of an app window's bar. Pushed on `webapp:state`. */
export interface WebAppState {
  appId: string
  name: string
  /** 64 px icon of the app (data URL). */
  icon: string
  url: string
  title: string
  themeColor?: string
  loading: boolean
  canGoBack: boolean
  /** False when the page has left the app's site (login, external link): the bar then shows its domain. */
  inScope: boolean
  hue: number
}

/**
 * Progress of a passkey request: waiting for the phone, a Bluetooth problem (`unsupported`: no scan on this system), then
 * phone found and connecting, confirming on the phone, or failed.
 */
export type PasskeyStatus = 'waiting' | 'off' | 'no-adapter' | 'denied' | 'error' | 'unsupported' | 'connecting' | 'confirm' | 'failed'

/** What the `#passkey` window shows: a passkey request waiting for a phone (null: nothing, the window closes). */
export interface PasskeyPromptState {
  type: 'get' | 'create'
  /** Relying party id: the site the passkey is for. */
  rpId: string
  /** QR code (SVG data URL). */
  qr: string
  status: PasskeyStatus
}

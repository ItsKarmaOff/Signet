// Contract between the extension API preload (src/preload/extension-apis.ts) and the main process
// (src/main/features/extensions/apis/). Pure data: no Electron import.

/** Calls from an extension context to main: `invoke(CRX_INVOKE, method, args[])`. */
export const CRX_INVOKE = 'signet-crx'

/** Events from main to an extension context: `send(CRX_EVENT, name, args[])`. */
export const CRX_EVENT = 'signet-crx-event'

/** Answer to a call; errors travel as data so that the message reaches the extension untouched. */
export type CrxResult = { ok: true; value?: unknown } | { ok: false; error: string }

/**
 * `chrome.privacy` settings other than WebRTC, grouped by namespace. They are stored and returned but have no effect
 * (Chromium has no matching switch we can flip from Electron); they exist so an extension that touches them does not crash.
 * The value is the default Chrome reports.
 */
export const PRIVACY_SETTINGS = {
  network: {
    networkPredictionEnabled: true,
    webRTCIPHandlingPolicy: 'default',
    // Deprecated in Chrome but still read by older extensions.
    webRTCMultipleRoutesEnabled: true,
    webRTCNonProxiedUdpEnabled: true
  },
  services: {
    alternateErrorPagesEnabled: true,
    autofillAddressEnabled: true,
    autofillCreditCardEnabled: true,
    autofillEnabled: true,
    hotwordSearchEnabled: false,
    passwordSavingEnabled: true,
    safeBrowsingEnabled: true,
    safeBrowsingExtendedReportingEnabled: false,
    searchSuggestEnabled: true,
    spellingServiceEnabled: false,
    translationServiceEnabled: true
  },
  websites: {
    adMeasurementEnabled: true,
    doNotTrackEnabled: false,
    fledgeEnabled: true,
    hyperlinkAuditingEnabled: true,
    protectedContentEnabled: true,
    referrersEnabled: true,
    relatedWebsiteSetsEnabled: true,
    thirdPartyCookiesAllowed: true,
    topicsEnabled: true
  }
} as const satisfies Record<string, Record<string, boolean | string>>

/** Methods of a ChromeSetting object (`chrome.proxy.settings`, `chrome.privacy.network.x`…). */
export const SETTING_METHODS = ['get', 'set', 'clear'] as const

/**
 * Namespaces the preload defines, with the manifest permissions that unlock each one (any of them).
 */
export const NAMESPACE_PERMISSIONS = {
  proxy: ['proxy'],
  privacy: ['privacy'],
  alarms: ['alarms'],
  offscreen: ['offscreen'],
  // webRequest is native in Electron; only onAuthRequired is wrapped (see the preload), under either permission.
  webRequest: ['webRequest', 'webRequestAuthProvider'],
  declarativeNetRequest: ['declarativeNetRequest', 'declarativeNetRequestWithHostAccess']
} as const satisfies Record<string, readonly string[]>

/** Internal call: the preload tells main how its installation went (logged in the terminal). */
export const INSTALLED_METHOD = '__installed'

/** Outcome of the preload in one extension context. */
export interface InstallReport {
  context: 'worker' | 'page'
  namespaces: string[]
  /** first: ran before the library; replaced-frozen: copied the library's frozen `chrome`; failed: nothing installed. */
  mode: 'first' | 'replaced-frozen' | 'failed'
  error?: string
}

/** Events per namespace (the setting `onChange` events are added from the setting list). */
export const ALARM_EVENT = 'alarms.onAlarm'
export const PROXY_ERROR_EVENT = 'proxy.onProxyError'

/** `chrome.offscreen.Reason` values (Chrome's list). */
export const OFFSCREEN_REASONS = [
  'TESTING',
  'AUDIO_PLAYBACK',
  'IFRAME_SCRIPTING',
  'DOM_SCRAPING',
  'BLOBS',
  'DOM_PARSER',
  'USER_MEDIA',
  'DISPLAY_MEDIA',
  'WEB_RTC',
  'CLIPBOARD',
  'LOCAL_STORAGE',
  'WORKERS',
  'BATTERY_STATUS',
  'MATCH_MEDIA',
  'GEOLOCATION'
] as const

/**
 * Whether a preload runs in an extension context. Strict on purpose: service-worker preloads run for EVERY worker of the
 * session, web sites' included, so only a `chrome-extension://` URL qualifies.
 * - worker: its script URL starts with chrome-extension://
 * - page: a chrome-extension:// document that is the top frame (subframes get nothing).
 */
export function isExtensionContext(input: { processType: string; url: string | null | undefined; isTopFrame: boolean }): boolean {
  if (!input.url?.startsWith('chrome-extension://')) return false
  return input.processType === 'service-worker' || input.isTopFrame
}

/** Event main sends to ask an extension for proxy credentials: args `[requestKey, details]`. Same name as Chrome's event. */
export const AUTH_EVENT = 'webRequest.onAuthRequired'

/** Call by which a context tells main how many onAuthRequired listeners it has now: args `[count]`. */
export const AUTH_LISTENERS_METHOD = 'webRequest.authListeners'

/** Call by which the extension's preload answers: args `[requestKey, response]` (Chrome's BlockingResponse). */
export const AUTH_RESPONSE_METHOD = 'webRequest.authResponse'

/**
 * `chrome.declarativeNetRequest` enums: name -> VALUE, and the values are NOT the names (Chrome uses camelCase and
 * lowercase strings; rules written with the names would never match).
 */
export const DNR_ENUMS = {
  RuleActionType: {
    BLOCK: 'block',
    REDIRECT: 'redirect',
    ALLOW: 'allow',
    UPGRADE_SCHEME: 'upgradeScheme',
    MODIFY_HEADERS: 'modifyHeaders',
    ALLOW_ALL_REQUESTS: 'allowAllRequests'
  },
  HeaderOperation: { APPEND: 'append', SET: 'set', REMOVE: 'remove' },
  ResourceType: {
    MAIN_FRAME: 'main_frame',
    SUB_FRAME: 'sub_frame',
    STYLESHEET: 'stylesheet',
    SCRIPT: 'script',
    IMAGE: 'image',
    FONT: 'font',
    OBJECT: 'object',
    XMLHTTPREQUEST: 'xmlhttprequest',
    PING: 'ping',
    CSP_REPORT: 'csp_report',
    MEDIA: 'media',
    WEBSOCKET: 'websocket',
    WEBTRANSPORT: 'webtransport',
    WEBBUNDLE: 'webbundle',
    OTHER: 'other'
  },
  RequestMethod: {
    CONNECT: 'connect',
    DELETE: 'delete',
    GET: 'get',
    HEAD: 'head',
    OPTIONS: 'options',
    PATCH: 'patch',
    POST: 'post',
    PUT: 'put',
    OTHER: 'other'
  },
  DomainType: { FIRST_PARTY: 'firstParty', THIRD_PARTY: 'thirdParty' },
  UnsupportedRegexReason: { SYNTAX_ERROR: 'syntaxError', MEMORY_LIMIT_EXCEEDED: 'memoryLimitExceeded' }
} as const

export const DNR_CONSTANTS = {
  MAX_NUMBER_OF_DYNAMIC_RULES: 30000,
  MAX_NUMBER_OF_UNSAFE_DYNAMIC_RULES: 5000,
  MAX_NUMBER_OF_SESSION_RULES: 5000,
  MAX_NUMBER_OF_UNSAFE_SESSION_RULES: 5000,
  GUARANTEED_MINIMUM_STATIC_RULES: 30000,
  MAX_NUMBER_OF_STATIC_RULESETS: 100,
  MAX_NUMBER_OF_ENABLED_STATIC_RULESETS: 50,
  MAX_NUMBER_OF_REGEX_RULES: 1000,
  DYNAMIC_RULESET_ID: '_dynamic',
  SESSION_RULESET_ID: '_session'
} as const

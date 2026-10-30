// Catalog of the Chrome extension API namespaces (names only, from https://developer.chrome.com/docs/extensions/reference/api).
// Pure data, no Electron import: it feeds the API probe (tests/extensions/api-probe) now, and the missing-API safety net of the
// extension preload later (so it must stay plain JSON-serialisable data).

export interface NamespaceSpec {
  /** Manifest permissions that unlock the namespace (any of them). `[]` = always available (runtime, tabs, windows...). */
  permissions: string[]
  /** Alternative unlock: a manifest key such as `side_panel`, `omnibox`, `commands`, `action`, `devtools_page`. */
  manifestKey?: string
  /** Function names. */
  methods: string[]
  /** Methods that should resolve a neutral value instead of failing when stubbed: `{ getAll: [], getFontList: [] }`. */
  soft?: Record<string, unknown>
  /** Event names (`onMoved`...). */
  events: string[]
  /** Events that also need `addRules` / `getRules` / `removeRules` (`declarativeContent.onPageChanged`). */
  declarativeEvents?: string[]
  /** Constants (`TAB_ID_NONE: -1`) and enums (`{ Color: { GREY: 'grey' } }`). */
  constants?: Record<string, unknown>
  /** Constructors that must exist (`declarativeContent.PageStateMatcher`...). */
  classes?: string[]
  /** Sub-namespaces: `system.cpu`, `storage.managed`, `devtools.panels`. */
  children?: Record<string, NamespaceSpec>
  /**
   * Members (methods or events) whose NATIVE implementation kills the whole browser process in Electron (it needs hardware
   * services Electron never starts). The safety net replaces them with inert stubs even though they exist natively.
   */
  crashes?: string[]
  /** Signet will not provide this namespace (see docs/EXTENSION-APIS.md); a stub is still fine. */
  unsupported?: true
  /** Only exists in content scripts, never stub it from the extension preload. */
  contentScriptOnly?: true
}

const ns = (s: Partial<NamespaceSpec> = {}): NamespaceSpec => ({ permissions: [], methods: [], events: [], ...s })
const unsupported = (s: Partial<NamespaceSpec> = {}): NamespaceSpec => ns({ ...s, unsupported: true })

/** `ChromeSetting` object (`proxy.settings`, `privacy.network.webRTCIPHandlingPolicy`...). */
const setting = (): NamespaceSpec => ns({ methods: ['get', 'set', 'clear'], events: ['onChange'] })
const settings = (permission: string, names: string[]): Record<string, NamespaceSpec> =>
  Object.fromEntries(names.map((n) => [n, { ...setting(), permissions: [permission] }]))

/** `ContentSetting` object (`contentSettings.cookies`...). */
const contentSetting = (): NamespaceSpec => ns({ permissions: ['contentSettings'], methods: ['clear', 'get', 'getResourceIdentifiers', 'set'] })

/** `StorageArea` object. */
const storageArea = (extra: Partial<NamespaceSpec> = {}): NamespaceSpec =>
  ns({ permissions: ['storage'], methods: ['clear', 'get', 'getBytesInUse', 'getKeys', 'remove', 'set'], events: ['onChanged'], ...extra })

const tabAction = [
  'getBadgeBackgroundColor',
  'getBadgeText',
  'getPopup',
  'getTitle',
  'setBadgeBackgroundColor',
  'setBadgeText',
  'setIcon',
  'setPopup',
  'setTitle'
]

export const EXTENSION_API_CATALOG: Record<string, NamespaceSpec> = {
  // ---- Core and high traffic ----
  action: ns({
    manifestKey: 'action',
    methods: [...tabAction, 'disable', 'enable', 'getBadgeTextColor', 'getUserSettings', 'isEnabled', 'openPopup', 'setBadgeTextColor'],
    events: ['onClicked', 'onUserSettingsChanged']
  }),
  alarms: ns({ permissions: ['alarms'], methods: ['clear', 'clearAll', 'create', 'get', 'getAll'], soft: { getAll: [] }, events: ['onAlarm'] }),
  commands: ns({ manifestKey: 'commands', methods: ['getAll'], soft: { getAll: [] }, events: ['onCommand'] }),
  contextMenus: ns({
    permissions: ['contextMenus'],
    methods: ['create', 'remove', 'removeAll', 'update'],
    events: ['onClicked'],
    constants: { ACTION_MENU_TOP_LEVEL_LIMIT: 6 }
  }),
  cookies: ns({ permissions: ['cookies'], methods: ['get', 'getAll', 'getAllCookieStores', 'remove', 'set'], events: ['onChanged'] }),
  declarativeNetRequest: ns({
    permissions: ['declarativeNetRequest', 'declarativeNetRequestWithHostAccess', 'declarativeNetRequestFeedback'],
    methods: [
      'getAvailableStaticRuleCount',
      'getDisabledRuleIds',
      'getDynamicRules',
      'getEnabledRulesets',
      'getMatchedRules',
      'getSessionRules',
      'isRegexSupported',
      'setExtensionActionOptions',
      'testMatchOutcome',
      'updateDynamicRules',
      'updateEnabledRulesets',
      'updateSessionRules',
      'updateStaticRules'
    ],
    events: ['onRuleMatchedDebug'],
    constants: {
      DYNAMIC_RULESET_ID: '_dynamic',
      SESSION_RULESET_ID: '_session',
      GETMATCHEDRULES_QUOTA_INTERVAL: 10,
      MAX_GETMATCHEDRULES_CALLS_PER_INTERVAL: 20,
      GUARANTEED_MINIMUM_STATIC_RULES: 30000,
      MAX_NUMBER_OF_DYNAMIC_RULES: 30000,
      MAX_NUMBER_OF_UNSAFE_DYNAMIC_RULES: 5000,
      MAX_NUMBER_OF_SESSION_RULES: 5000,
      MAX_NUMBER_OF_UNSAFE_SESSION_RULES: 5000,
      MAX_NUMBER_OF_REGEX_RULES: 1000,
      MAX_NUMBER_OF_STATIC_RULESETS: 100,
      MAX_NUMBER_OF_ENABLED_STATIC_RULESETS: 50
    }
  }),
  downloads: ns({
    permissions: ['downloads', 'downloads.open', 'downloads.ui', 'downloads.shelf'],
    methods: [
      'acceptDanger',
      'cancel',
      'download',
      'erase',
      'getFileIcon',
      'open',
      'pause',
      'removeFile',
      'resume',
      'search',
      'setShelfEnabled',
      'setUiOptions',
      'show',
      'showDefaultFolder'
    ],
    soft: { search: [] },
    events: ['onChanged', 'onCreated', 'onDeterminingFilename', 'onErased']
  }),
  i18n: ns({ methods: ['detectLanguage', 'getAcceptLanguages', 'getMessage', 'getUILanguage'] }),
  notifications: ns({
    permissions: ['notifications'],
    methods: ['clear', 'create', 'getAll', 'getPermissionLevel', 'update'],
    soft: { getAll: {} },
    events: ['onButtonClicked', 'onClicked', 'onClosed', 'onPermissionLevelChanged', 'onShowSettings']
  }),
  offscreen: ns({ permissions: ['offscreen'], methods: ['closeDocument', 'createDocument', 'hasDocument'] }),
  permissions: ns({
    methods: ['addHostAccessRequest', 'contains', 'getAll', 'remove', 'removeHostAccessRequest', 'request'],
    events: ['onAdded', 'onRemoved']
  }),
  privacy: ns({
    permissions: ['privacy'],
    children: {
      network: ns({
        permissions: ['privacy'],
        children: settings('privacy', ['networkPredictionEnabled', 'webRTCIPHandlingPolicy', 'webRTCMultipleRoutesEnabled', 'webRTCNonProxiedUdpEnabled'])
      }),
      services: ns({
        permissions: ['privacy'],
        children: settings('privacy', [
          'alternateErrorPagesEnabled',
          'autofillAddressEnabled',
          'autofillCreditCardEnabled',
          'autofillEnabled',
          'hotwordSearchEnabled',
          'passwordSavingEnabled',
          'safeBrowsingEnabled',
          'safeBrowsingExtendedReportingEnabled',
          'searchSuggestEnabled',
          'spellingServiceEnabled',
          'translationServiceEnabled'
        ])
      }),
      websites: ns({
        permissions: ['privacy'],
        children: settings('privacy', [
          'adMeasurementEnabled',
          'doNotTrackEnabled',
          'fledgeEnabled',
          'hyperlinkAuditingEnabled',
          'protectedContentEnabled',
          'referrersEnabled',
          'relatedWebsiteSetsEnabled',
          'thirdPartyCookiesAllowed',
          'topicsEnabled'
        ])
      })
    }
  }),
  proxy: ns({ permissions: ['proxy'], events: ['onProxyError'], children: { settings: { ...setting(), permissions: ['proxy'] } } }),
  runtime: ns({
    methods: [
      'connect',
      'connectNative',
      'getBackgroundPage',
      'getContexts',
      'getManifest',
      'getPackageDirectoryEntry',
      'getPlatformInfo',
      'getURL',
      'openOptionsPage',
      'reload',
      'requestUpdateCheck',
      'restart',
      'restartAfterDelay',
      'sendMessage',
      'sendNativeMessage',
      'setUninstallURL'
    ],
    events: [
      'onBrowserUpdateAvailable',
      'onConnect',
      'onConnectExternal',
      'onConnectNative',
      'onInstalled',
      'onMessage',
      'onMessageExternal',
      'onRestartRequired',
      'onStartup',
      'onSuspend',
      'onSuspendCanceled',
      'onUpdateAvailable',
      'onUserScriptConnect',
      'onUserScriptMessage'
    ],
    constants: {
      id: '',
      ContextType: {},
      OnInstalledReason: {},
      OnRestartRequiredReason: {},
      PlatformArch: {},
      PlatformNaclArch: {},
      PlatformOs: {},
      RequestUpdateCheckStatus: {}
    }
  }),
  scripting: ns({
    permissions: ['scripting'],
    methods: [
      'executeScript',
      'getRegisteredContentScripts',
      'insertCSS',
      'registerContentScripts',
      'removeCSS',
      'unregisterContentScripts',
      'updateContentScripts'
    ],
    constants: { ExecutionWorld: {}, StyleOrigin: {} }
  }),
  storage: ns({
    permissions: ['storage'],
    events: ['onChanged'],
    children: {
      local: storageArea({ constants: { QUOTA_BYTES: 10485760 }, methods: ['clear', 'get', 'getBytesInUse', 'getKeys', 'remove', 'set', 'setAccessLevel'] }),
      sync: storageArea({
        constants: {
          QUOTA_BYTES: 102400,
          QUOTA_BYTES_PER_ITEM: 8192,
          MAX_ITEMS: 512,
          MAX_WRITE_OPERATIONS_PER_HOUR: 1800,
          MAX_WRITE_OPERATIONS_PER_MINUTE: 120
        }
      }),
      session: storageArea({ constants: { QUOTA_BYTES: 10485760 }, methods: ['clear', 'get', 'getBytesInUse', 'getKeys', 'remove', 'set', 'setAccessLevel'] }),
      managed: storageArea({ methods: ['get', 'getBytesInUse', 'getKeys'] })
    }
  }),
  tabs: ns({
    methods: [
      'captureVisibleTab',
      'connect',
      'create',
      'detectLanguage',
      'discard',
      'duplicate',
      'get',
      'getCurrent',
      'getZoom',
      'getZoomSettings',
      'goBack',
      'goForward',
      'group',
      'highlight',
      'insertCSS',
      'move',
      'query',
      'reload',
      'remove',
      'removeCSS',
      'sendMessage',
      'setZoom',
      'setZoomSettings',
      'toggleReaderMode',
      'ungroup',
      'update'
    ],
    events: ['onActivated', 'onAttached', 'onCreated', 'onDetached', 'onHighlighted', 'onMoved', 'onRemoved', 'onReplaced', 'onUpdated', 'onZoomChange'],
    constants: {
      TAB_ID_NONE: -1,
      TAB_INDEX_NONE: -1,
      MAX_CAPTURE_VISIBLE_TAB_CALLS_PER_SECOND: 2,
      TabStatus: {},
      WindowType: {},
      ZoomSettingsMode: {},
      ZoomSettingsScope: {}
    }
  }),
  webNavigation: ns({
    permissions: ['webNavigation'],
    methods: ['getAllFrames', 'getFrame'],
    events: [
      'onBeforeNavigate',
      'onCommitted',
      'onCompleted',
      'onCreatedNavigationTarget',
      'onDOMContentLoaded',
      'onErrorOccurred',
      'onHistoryStateUpdated',
      'onReferenceFragmentUpdated',
      'onTabReplaced'
    ]
  }),
  webRequest: ns({
    permissions: ['webRequest', 'webRequestBlocking', 'webRequestAuthProvider'],
    methods: ['handlerBehaviorChanged'],
    events: [
      'onActionIgnored',
      'onAuthRequired',
      'onBeforeRedirect',
      'onBeforeRequest',
      'onBeforeSendHeaders',
      'onCompleted',
      'onErrorOccurred',
      'onHeadersReceived',
      'onResponseStarted',
      'onSendHeaders'
    ],
    constants: { MAX_HANDLER_BEHAVIOR_CHANGED_CALLS_PER_10_MINUTES: 20 }
  }),
  windows: ns({
    methods: ['create', 'get', 'getAll', 'getCurrent', 'getLastFocused', 'remove', 'update'],
    events: ['onBoundsChanged', 'onCreated', 'onFocusChanged', 'onRemoved'],
    constants: { WINDOW_ID_NONE: -1, WINDOW_ID_CURRENT: -2, CreateType: {}, WindowState: {}, WindowType: {} }
  }),

  // ---- Medium relevance ----
  bookmarks: ns({
    permissions: ['bookmarks'],
    methods: ['create', 'get', 'getChildren', 'getRecent', 'getSubTree', 'getTree', 'move', 'remove', 'removeTree', 'search', 'update'],
    events: ['onChanged', 'onChildrenReordered', 'onCreated', 'onImportBegan', 'onImportEnded', 'onMoved', 'onRemoved'],
    constants: { MAX_WRITE_OPERATIONS_PER_HOUR: 1000000, MAX_SUSTAINED_WRITE_OPERATIONS_PER_MINUTE: 1000000 }
  }),
  browsingData: ns({
    permissions: ['browsingData'],
    methods: [
      'remove',
      'removeAppcache',
      'removeCache',
      'removeCacheStorage',
      'removeCookies',
      'removeDownloads',
      'removeFileSystems',
      'removeFormData',
      'removeHistory',
      'removeIndexedDB',
      'removeLocalStorage',
      'removePasswords',
      'removePluginData',
      'removeServiceWorkers',
      'removeWebSQL',
      'settings'
    ]
  }),
  contentSettings: ns({
    permissions: ['contentSettings'],
    children: Object.fromEntries(
      [
        'automaticDownloads',
        'autoVerify',
        'camera',
        'clipboard',
        'cookies',
        'fullscreen',
        'images',
        'javascript',
        'location',
        'microphone',
        'mouselock',
        'notifications',
        'plugins',
        'popups',
        'unsandboxedPlugins'
      ].map((n) => [n, contentSetting()])
    )
  }),
  declarativeContent: ns({
    permissions: ['declarativeContent'],
    events: ['onPageChanged'],
    declarativeEvents: ['onPageChanged'],
    classes: ['PageStateMatcher', 'RequestContentScript', 'SetIcon', 'ShowAction', 'ShowPageAction']
  }),
  desktopCapture: ns({ permissions: ['desktopCapture'], methods: ['cancelChooseDesktopMedia', 'chooseDesktopMedia'] }),
  devtools: ns({
    manifestKey: 'devtools_page',
    children: {
      inspectedWindow: ns({
        manifestKey: 'devtools_page',
        methods: ['eval', 'getResources', 'reload'],
        events: ['onResourceAdded', 'onResourceContentCommitted'],
        constants: { tabId: 0 }
      }),
      network: ns({ manifestKey: 'devtools_page', methods: ['getHAR'], events: ['onNavigated', 'onRequestFinished'] }),
      panels: ns({
        manifestKey: 'devtools_page',
        methods: ['create', 'openResource', 'setOpenResourceHandler'],
        constants: { themeName: '', elements: {}, sources: {} }
      }),
      performance: ns({ manifestKey: 'devtools_page', events: ['onProfilingStarted', 'onProfilingStopped'] }),
      recorder: ns({ manifestKey: 'devtools_page', methods: ['registerRecorderExtensionPlugin'] })
    }
  }),
  dns: ns({ permissions: ['dns'], methods: ['resolve'] }),
  dom: ns({ methods: ['openOrClosedShadowRoot'], contentScriptOnly: true }),
  extension: ns({
    methods: ['getBackgroundPage', 'getURL', 'getViews', 'isAllowedFileSchemeAccess', 'isAllowedIncognitoAccess'],
    soft: { getViews: [] },
    constants: { inIncognitoContext: false, ViewType: {} }
  }),
  fontSettings: ns({
    permissions: ['fontSettings'],
    methods: [
      'clearDefaultFixedFontSize',
      'clearDefaultFontSize',
      'clearFont',
      'clearMinimumFontSize',
      'getDefaultFixedFontSize',
      'getDefaultFontSize',
      'getFont',
      'getFontList',
      'getMinimumFontSize',
      'setDefaultFixedFontSize',
      'setDefaultFontSize',
      'setFont',
      'setMinimumFontSize'
    ],
    soft: { getFontList: [] },
    events: ['onDefaultFixedFontSizeChanged', 'onDefaultFontSizeChanged', 'onFontChanged', 'onMinimumFontSizeChanged']
  }),
  history: ns({
    permissions: ['history'],
    methods: ['addUrl', 'deleteAll', 'deleteRange', 'deleteUrl', 'getVisits', 'search'],
    soft: { search: [], getVisits: [] },
    events: ['onVisitRemoved', 'onVisited']
  }),
  identity: ns({
    permissions: ['identity'],
    methods: ['clearAllCachedAuthTokens', 'getAccounts', 'getAuthToken', 'getProfileUserInfo', 'getRedirectURL', 'launchWebAuthFlow', 'removeCachedAuthToken'],
    events: ['onSignInChanged']
  }),
  idle: ns({ permissions: ['idle'], methods: ['queryState', 'setDetectionInterval'], events: ['onStateChanged'] }),
  management: ns({
    permissions: ['management'],
    methods: [
      'createAppShortcut',
      'generateAppForLink',
      'get',
      'getAll',
      'getPermissionWarningsById',
      'getPermissionWarningsByManifest',
      'getSelf',
      'installReplacementWebApp',
      'launchApp',
      'setEnabled',
      'setLaunchType',
      'uninstall',
      'uninstallSelf'
    ],
    events: ['onDisabled', 'onEnabled', 'onInstalled', 'onUninstalled']
  }),
  omnibox: ns({
    manifestKey: 'omnibox',
    methods: ['setDefaultSuggestion'],
    events: ['onDeleteSuggestion', 'onInputCancelled', 'onInputChanged', 'onInputEntered', 'onInputStarted']
  }),
  pageCapture: ns({ permissions: ['pageCapture'], methods: ['saveAsMHTML'] }),
  power: ns({ permissions: ['power'], methods: ['releaseKeepAwake', 'requestKeepAwake', 'reportActivity'] }),
  printerProvider: ns({
    permissions: ['printerProvider'],
    events: ['onGetCapabilityRequested', 'onGetPrintersRequested', 'onGetUsbPrinterInfoRequested', 'onPrintRequested'],
    // Adding a listener to these crashed the process in the API probe (onGetUsbPrinterInfoRequested measured, the others by precaution).
    crashes: ['onGetCapabilityRequested', 'onGetPrintersRequested', 'onGetUsbPrinterInfoRequested', 'onPrintRequested']
  }),
  // Added in Chrome 153; member names unverified (the probe reports what Chromium 156 exposes).
  publicSuffix: ns({ permissions: ['publicSuffix'] }),
  search: ns({ permissions: ['search'], methods: ['query'] }),
  sessions: ns({
    permissions: ['sessions'],
    methods: ['getDevices', 'getRecentlyClosed', 'restore'],
    soft: { getDevices: [], getRecentlyClosed: [] },
    events: ['onChanged'],
    constants: { MAX_SESSION_RESULTS: 25 }
  }),
  sidePanel: ns({
    permissions: ['sidePanel'],
    manifestKey: 'side_panel',
    methods: ['close', 'getOptions', 'getPanelBehavior', 'open', 'setOptions', 'setPanelBehavior'],
    events: ['onClosed', 'onOpened']
  }),
  system: ns({
    children: {
      cpu: ns({ permissions: ['system.cpu'], methods: ['getInfo'] }),
      display: ns({
        permissions: ['system.display'],
        methods: [
          'clearTouchCalibration',
          'completeCustomTouchCalibration',
          'getDisplayLayout',
          'getInfo',
          'overscanCalibrationAdjust',
          'overscanCalibrationComplete',
          'overscanCalibrationReset',
          'overscanCalibrationStart',
          'setDisplayProperties',
          'setMirrorMode',
          'showNativeTouchCalibration',
          'startCustomTouchCalibration'
        ],
        events: ['onDisplayChanged']
      }),
      memory: ns({ permissions: ['system.memory'], methods: ['getInfo'] }),
      storage: ns({
        permissions: ['system.storage'],
        methods: ['ejectDevice', 'getAvailableCapacity', 'getInfo'],
        soft: { getInfo: [] },
        events: ['onAttached', 'onDetached'],
        // Adding a listener to onAttached / onDetached segfaulted the process in the API probe; the calls are replaced too (unmeasured).
        crashes: ['ejectDevice', 'getAvailableCapacity', 'getInfo', 'onAttached', 'onDetached']
      })
    }
  }),
  tabGroups: ns({
    permissions: ['tabGroups'],
    methods: ['get', 'move', 'query', 'update'],
    events: ['onCreated', 'onMoved', 'onRemoved', 'onUpdated'],
    constants: { TAB_GROUP_ID_NONE: -1, Color: {} }
  }),
  topSites: ns({ permissions: ['topSites'], methods: ['get'], soft: { get: [] } }),
  tts: ns({
    permissions: ['tts'],
    methods: ['getVoices', 'isSpeaking', 'pause', 'resume', 'speak', 'stop'],
    soft: { getVoices: [], isSpeaking: false },
    events: ['onEvent']
  }),
  userScripts: ns({
    permissions: ['userScripts'],
    methods: ['configureWorld', 'execute', 'getScripts', 'getWorldConfigurations', 'register', 'resetWorldConfiguration', 'unregister', 'update'],
    constants: { ExecutionWorld: {} }
  }),

  // ---- Low relevance or not applicable (never provided, see docs/EXTENSION-APIS.md) ----
  accessibilityFeatures: unsupported({
    permissions: ['accessibilityFeatures.read', 'accessibilityFeatures.modify'],
    children: settings('accessibilityFeatures.read', [
      'animationPolicy',
      'autoclick',
      'caretHighlight',
      'cursorColor',
      'cursorHighlight',
      'dictation',
      'dockedMagnifier',
      'focusHighlight',
      'highContrast',
      'largeCursor',
      'screenMagnifier',
      'selectToSpeak',
      'spokenFeedback',
      'stickyKeys',
      'switchAccess',
      'virtualKeyboard'
    ])
  }),
  audio: unsupported({
    permissions: ['audio'],
    methods: ['getDevices', 'getMute', 'setActiveDevices', 'setMute', 'setProperties'],
    events: ['onDeviceListChanged', 'onLevelChanged', 'onMuteChanged']
  }),
  browserAction: unsupported({
    manifestKey: 'browser_action',
    methods: [...tabAction, 'disable', 'enable', 'getBadgeTextColor', 'openPopup', 'setBadgeTextColor'],
    events: ['onClicked']
  }),
  certificateProvider: unsupported({
    permissions: ['certificateProvider'],
    methods: ['reportSignature', 'requestPin', 'setCertificates', 'stopPinRequest'],
    events: ['onCertificatesUpdateRequested', 'onSignatureRequested']
  }),
  // ChromeOS only (clipboardRead / clipboardWrite permissions are separate); Chromium still exposes the object, the probe sees it on `chrome`.
  clipboard: unsupported({ permissions: ['clipboard'], methods: ['setImageData'], events: ['onClipboardDataChanged'] }),
  debugger: unsupported({
    permissions: ['debugger'],
    methods: ['attach', 'detach', 'getTargets', 'sendCommand'],
    events: ['onDetach', 'onEvent']
  }),
  documentScan: unsupported({
    permissions: ['documentScan'],
    methods: ['cancelScan', 'closeScanner', 'getOptionGroups', 'getScannerList', 'openScanner', 'readScanData', 'scan', 'setOptions', 'startScan']
  }),
  enterprise: unsupported({
    children: {
      deviceAttributes: unsupported({
        permissions: ['enterprise.deviceAttributes'],
        methods: ['getDeviceAnnotatedLocation', 'getDeviceAssetId', 'getDeviceHostname', 'getDeviceSerialNumber', 'getDirectoryDeviceId']
      }),
      hardwarePlatform: unsupported({ permissions: ['enterprise.hardwarePlatform'], methods: ['getHardwarePlatformInfo'] }),
      login: unsupported({ permissions: ['enterprise.login'], methods: ['exitCurrentManagedGuestSession'] }),
      networkingAttributes: unsupported({ permissions: ['enterprise.networkingAttributes'], methods: ['getNetworkDetails'] }),
      platformKeys: unsupported({
        permissions: ['enterprise.platformKeys'],
        methods: ['challengeKey', 'challengeMachineKey', 'challengeUserKey', 'getCertificates', 'getToken', 'importCertificate', 'removeCertificate']
      })
    }
  }),
  fileBrowserHandler: unsupported({ permissions: ['fileBrowserHandler'], methods: ['selectFile'], events: ['onExecute'] }),
  fileSystemProvider: unsupported({
    permissions: ['fileSystemProvider'],
    methods: ['get', 'getAll', 'mount', 'notify', 'unmount'],
    events: [
      'onAbortRequested',
      'onGetMetadataRequested',
      'onMountRequested',
      'onOpenFileRequested',
      'onReadDirectoryRequested',
      'onReadFileRequested',
      'onUnmountRequested'
    ]
  }),
  gcm: unsupported({
    permissions: ['gcm'],
    methods: ['register', 'send', 'unregister'],
    events: ['onMessage', 'onMessagesDeleted', 'onSendError'],
    constants: { MAX_MESSAGE_SIZE: 4096 }
  }),
  input: unsupported({
    children: {
      ime: unsupported({
        permissions: ['input'],
        methods: [
          'clearComposition',
          'commitText',
          'deleteSurroundingText',
          'hideInputView',
          'sendKeyEvents',
          'setCandidateWindowProperties',
          'setCandidates',
          'setComposition',
          'setCursorPosition',
          'setMenuItems',
          'updateMenuItems'
        ],
        events: [
          'onActivate',
          'onBlur',
          'onCandidateClicked',
          'onDeactivated',
          'onFocus',
          'onInputContextUpdate',
          'onKeyEvent',
          'onMenuItemActivated',
          'onReset',
          'onSurroundingTextChanged'
        ]
      })
    }
  }),
  instanceID: unsupported({
    permissions: ['gcm'],
    methods: ['deleteID', 'deleteToken', 'getCreationTime', 'getID', 'getToken'],
    events: ['onTokenRefresh']
  }),
  loginState: unsupported({ permissions: ['loginState'], methods: ['getProfileType', 'getSessionState'], events: ['onSessionStateChanged'] }),
  mimeHandler: unsupported({ manifestKey: 'mime_types_handler' }),
  pageAction: unsupported({
    manifestKey: 'page_action',
    methods: ['getPopup', 'getTitle', 'hide', 'setIcon', 'setPopup', 'setTitle', 'show'],
    events: ['onClicked']
  }),
  platformKeys: unsupported({
    permissions: ['platformKeys'],
    methods: ['getKeyPair', 'getKeyPairBySpki', 'selectClientCertificates', 'subtleCrypto', 'verifyTLSServerCertificate']
  }),
  printing: unsupported({
    permissions: ['printing'],
    methods: ['cancelJob', 'getPrinterInfo', 'getPrinters', 'submitJob'],
    events: ['onJobStatusChanged'],
    constants: { MAX_GET_PRINTER_INFO_CALLS_PER_MINUTE: 20, MAX_SUBMIT_JOB_CALLS_PER_MINUTE: 40 }
  }),
  printingMetrics: unsupported({ permissions: ['printingMetrics'], methods: ['getPrintJobs'], events: ['onPrintJobFinished'] }),
  processes: unsupported({
    permissions: ['processes'],
    methods: ['getProcessIdForTab', 'getProcessInfo', 'terminate'],
    events: ['onCreated', 'onExited', 'onUnresponsive', 'onUpdated', 'onUpdatedWithMemory']
  }),
  readingList: unsupported({
    permissions: ['readingList'],
    methods: ['addEntry', 'query', 'removeEntry', 'updateEntry'],
    events: ['onEntryAdded', 'onEntryRemoved', 'onEntryUpdated']
  }),
  systemLog: unsupported({ permissions: ['systemLog'], methods: ['add'] }),
  tabCapture: unsupported({
    permissions: ['tabCapture'],
    methods: ['capture', 'getCapturedTabs', 'getMediaStreamId'],
    events: ['onStatusChanged']
  }),
  ttsEngine: unsupported({
    permissions: ['ttsEngine'],
    manifestKey: 'tts_engine',
    methods: ['sendTtsAudio', 'sendTtsEvent', 'updateLanguage', 'updateVoices'],
    events: [
      'onInstallLanguageRequest',
      'onLanguageStatusRequest',
      'onPause',
      'onResume',
      'onSpeak',
      'onSpeakWithAudioStream',
      'onStop',
      'onUninstallLanguageRequest'
    ]
  }),
  vpnProvider: unsupported({
    permissions: ['vpnProvider'],
    methods: ['createConfig', 'destroyConfig', 'notifyConnectionStateChanged', 'sendPacket', 'setParameters'],
    events: ['onConfigCreated', 'onConfigRemoved', 'onPacketReceived', 'onPlatformMessage', 'onUIEvent']
  }),
  wallpaper: unsupported({ permissions: ['wallpaper'], methods: ['setWallpaper'] }),
  webAuthenticationProxy: unsupported({
    permissions: ['webAuthenticationProxy'],
    methods: ['attach', 'completeCreateRequest', 'completeGetRequest', 'completeIsUvpaaRequest', 'detach'],
    events: ['onCreateRequest', 'onGetRequest', 'onIsUvpaaRequest', 'onRemoteSessionStateChange', 'onRequestCanceled']
  })
}

/** Namespaces that exist in the Chrome reference but carry no member to provide (type-only). */
export const TYPE_ONLY_NAMESPACES: readonly string[] = ['events', 'types', 'extensionTypes']

/** Every namespace path in the catalog, children included (`system.cpu`, `storage.session`...). */
export function catalogPaths(catalog: Record<string, NamespaceSpec> = EXTENSION_API_CATALOG, prefix = ''): string[] {
  return Object.entries(catalog).flatMap(([name, spec]) => [`${prefix}${name}`, ...catalogPaths(spec.children ?? {}, `${prefix}${name}.`)])
}

/** Every manifest permission named by the catalog, sorted. */
export function catalogPermissions(catalog: Record<string, NamespaceSpec> = EXTENSION_API_CATALOG): string[] {
  const out = new Set<string>()
  const walk = (c: Record<string, NamespaceSpec>) => {
    for (const spec of Object.values(c)) {
      spec.permissions.forEach((p) => out.add(p))
      walk(spec.children ?? {})
    }
  }
  walk(catalog)
  return [...out].sort()
}

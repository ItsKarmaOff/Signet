/** `chrome.runtime.ExtensionContext`. */
export interface ExtensionContext {
  contextType: 'TAB' | 'POPUP' | 'BACKGROUND' | 'OFFSCREEN_DOCUMENT' | 'SIDE_PANEL' | 'DEVELOPER_TOOLS'
  contextId: string
  tabId: number
  windowId: number
  documentId?: string
  frameId: number
  documentUrl: string
  documentOrigin: string
  incognito: boolean
}

/** `chrome.runtime.ContextFilter`. */
export interface ContextFilter {
  contextTypes?: string[]
  contextIds?: string[]
  tabIds?: number[]
  windowIds?: number[]
  documentIds?: string[]
  frameIds?: number[]
  documentUrls?: string[]
  documentOrigins?: string[]
  incognito?: boolean
}

/** Context of the extension's service worker. */
export function backgroundContext(extensionId: string, workerUrl: string): ExtensionContext {
  return {
    contextType: 'BACKGROUND',
    contextId: `${extensionId}-background`,
    tabId: -1,
    windowId: -1,
    frameId: -1,
    documentUrl: workerUrl,
    documentOrigin: `chrome-extension://${extensionId}`,
    incognito: false
  }
}

/** Context of an open extension page (popup, options tab...). Phase 2 adds OFFSCREEN_DOCUMENT with the same shape. */
export function pageContext(extensionId: string, webContentsId: number, url: string, type: ExtensionContext['contextType'] = 'TAB'): ExtensionContext {
  return {
    contextType: type,
    contextId: `${extensionId}-page-${webContentsId}`,
    tabId: -1,
    windowId: -1,
    documentId: `${webContentsId}`,
    frameId: 0,
    documentUrl: url,
    documentOrigin: `chrome-extension://${extensionId}`,
    incognito: false
  }
}

const matches = <T>(wanted: T[] | undefined, value: T | undefined) => !Array.isArray(wanted) || (value !== undefined && wanted.includes(value))

/** Applies a ContextFilter like Chrome: every given property must match; a missing filter keeps all. */
export function filterContexts(contexts: ExtensionContext[], filter: unknown): ExtensionContext[] {
  if (!filter || typeof filter !== 'object') return contexts
  const f = filter as ContextFilter
  return contexts.filter(
    (c) =>
      matches(f.contextTypes, c.contextType) &&
      matches(f.contextIds, c.contextId) &&
      matches(f.tabIds, c.tabId) &&
      matches(f.windowIds, c.windowId) &&
      matches(f.documentIds, c.documentId) &&
      matches(f.frameIds, c.frameId) &&
      matches(f.documentUrls, c.documentUrl) &&
      matches(f.documentOrigins, c.documentOrigin) &&
      (typeof f.incognito !== 'boolean' || f.incognito === c.incognito)
  )
}

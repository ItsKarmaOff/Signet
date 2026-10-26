// API exposed by the preload (extensions library) in the page: action state, subscription, activation.
interface BrowserActionInfo {
  id: string
  title?: string
  text?: string
  color?: string
  popup?: string
  iconModified?: number
  tabs: Record<string, { title?: string; text?: string; color?: string; popup?: string }>
}
interface BrowserActionState {
  activeTabId?: number
  actions: BrowserActionInfo[]
}
interface Window {
  browserAction?: {
    getState(partition: string): Promise<BrowserActionState>
    addEventListener(name: 'update', listener: (state: BrowserActionState) => void): void
    removeEventListener(name: 'update', listener: (state: BrowserActionState) => void): void
    addObserver(partition: string): void
    removeObserver(partition: string): void
    activate(
      partition: string,
      details: {
        eventType: 'click' | 'contextmenu'
        extensionId: string
        tabId: number
        alignment?: string
        anchorRect: { x: number; y: number; width: number; height: number }
      }
    ): Promise<unknown>
  }
}

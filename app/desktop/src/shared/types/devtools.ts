/** State of the header of the detached devtools window (route `#devtools`). */
export interface DevToolsWindowState {
  tabId: string
  /** Host of the inspected page. */
  site: string
  /** False if the tab is not in the main window (Little Signet, app, private window, Peek): no docked devtools. */
  dockable: boolean
}

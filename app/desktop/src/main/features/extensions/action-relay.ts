/** How long the toolbar has to answer before main opens the popup itself. */
export const ACK_TIMEOUT_MS = 200

export interface ActionRelayDeps {
  /** Asks the toolbar of the right window to open the extension's button; false when there is no such toolbar. */
  send(token: string, extensionId: string): boolean
}

/**
 * An extension shortcut that must open a button's popup: the toolbar knows where the button is, so it is asked first.
 * No acknowledgement in time (toolbar hidden, compact mode, window without one) runs `fallback` instead.
 */
export class ActionRelay {
  private pending = new Map<string, ReturnType<typeof setTimeout>>()
  private count = 0

  constructor(private d: ActionRelayDeps) {}

  request(extensionId: string, fallback: () => void): void {
    const token = `a${++this.count}`
    if (!this.d.send(token, extensionId)) return fallback()
    this.pending.set(
      token,
      setTimeout(() => {
        this.pending.delete(token)
        fallback()
      }, ACK_TIMEOUT_MS)
    )
  }

  /** The toolbar takes the request. True once per token, and only before the timeout. */
  ack(token: string): boolean {
    const timer = this.pending.get(token)
    if (!timer) return false
    clearTimeout(timer)
    this.pending.delete(token)
    return true
  }
}

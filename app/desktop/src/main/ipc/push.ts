import type { WebContents } from 'electron'
import type { UiEventMap } from '@shared/ipc/contract'

/**
 * Pushes a message to a UI view. The channel and arguments come from the contract (`UiEventMap`):
 * an unknown channel or a wrongly typed payload does not compile. No effect if the view is already destroyed.
 */
export function pushUi<C extends keyof UiEventMap>(wc: WebContents, channel: C, ...args: UiEventMap[C]): void {
  if (!wc.isDestroyed()) wc.send(channel, ...args)
}

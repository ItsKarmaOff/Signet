// mDNS announcement (`_signet._tcp`): the phone finds the computer on the local network.
import Bonjour from 'bonjour-service'
import { LINK_PROTOCOL_VERSION, LINK_SERVICE_TYPE } from '@signet/shared/link/protocol'

export class Discovery {
  private bonjour: Bonjour | null = null
  private key = ''

  /** Announces (or re-announces if the name or port changed). */
  publish(name: string, id: string, port: number): void {
    const key = `${name}\n${id}\n${port}`
    if (this.bonjour && key === this.key) return
    this.unpublish()
    try {
      const bonjour = new Bonjour()
      bonjour.publish({ name, type: LINK_SERVICE_TYPE, port, txt: { v: String(LINK_PROTOCOL_VERSION), id } })
      this.bonjour = bonjour
      this.key = key
    } catch {
      // No multicast network: the phone falls back to the addresses in the QR code.
      this.bonjour = null
    }
  }

  unpublish(): void {
    const b = this.bonjour
    this.bonjour = null
    this.key = ''
    if (!b) return
    try {
      b.unpublishAll(() => b.destroy())
    } catch {
      /* already stopped */
    }
  }
}

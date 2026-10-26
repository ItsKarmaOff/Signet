// Links with phones: `link.json` file in the app folder. Each link's private key is encrypted
// with the system keychain (like passwords); of the token, only the SHA-256 is kept.
import { createHash, randomBytes, timingSafeEqual } from 'node:crypto'
import { readFileSync } from 'node:fs'
import type { LinkDevice, LinkPlatform } from '@signet/shared/link/protocol'
import { AtomicWriter } from '../../core/atomic'
import type { CertPair } from './certs'

/** Secret encryption. In production: `safeStorage`. */
export interface Vault {
  available(): boolean
  encrypt(text: string): string
  decrypt(data: string): string | null
}

export interface StoredLink {
  linkId: string
  peer: LinkDevice
  /** This device's switch, on this computer. */
  enabled: boolean
  certPem: string
  keyPem: string
  /** SHA-256 of the token, in hexadecimal. */
  tokenHash: string
  createdAt: number
  lastSeen: number | null
  /** Last "paused" state received from the phone. */
  peerPaused: boolean
}

interface FileLink extends Omit<StoredLink, 'keyPem'> {
  /** Encrypted key (`enc:`) or, without a keychain, in plain text (`raw:`). */
  key: string
}

interface FileData {
  version: 1
  desktopId: string
  general: boolean
  defaultCert?: { certPem: string; key: string }
  links: FileLink[]
}

export const hashToken = (token: string): string => createHash('sha256').update(token).digest('hex')

/** Constant-time comparison of the received token and the stored SHA-256. */
export function tokenMatches(token: string, tokenHash: string): boolean {
  const a = Buffer.from(hashToken(token), 'hex')
  const b = Buffer.from(tokenHash, 'hex')
  return a.length === b.length && timingSafeEqual(a, b)
}

export const newToken = (): string => randomBytes(32).toString('base64url')

const PLATFORMS: readonly LinkPlatform[] = ['macos', 'windows', 'linux', 'ios', 'android']
const isLink = (l: unknown): l is FileLink => {
  const x = l as FileLink
  return (
    !!x &&
    typeof x.linkId === 'string' &&
    typeof x.key === 'string' &&
    typeof x.certPem === 'string' &&
    typeof x.tokenHash === 'string' &&
    !!x.peer &&
    typeof x.peer.id === 'string' &&
    typeof x.peer.name === 'string' &&
    PLATFORMS.includes(x.peer.platform)
  )
}

export class LinkStore {
  desktopId = randomBytes(16).toString('hex')
  /** Master switch. Without a link, nothing listens anyway. */
  general = true
  defaultCert: CertPair | null = null
  links: StoredLink[] = []
  /** Unreadable file: the existing file is not overwritten during the session. */
  private blocked = false
  private writer: AtomicWriter

  constructor(
    private file: string,
    private vault: Vault
  ) {
    this.writer = new AtomicWriter(this.file, () => (this.blocked ? null : JSON.stringify(this.serialize())))
    this.load()
  }

  private seal(text: string): string {
    return this.vault.available() ? `enc:${this.vault.encrypt(text)}` : `raw:${text}`
  }

  private open(data: string): string | null {
    if (data.startsWith('raw:')) return data.slice(4)
    if (data.startsWith('enc:')) {
      try {
        return this.vault.decrypt(data.slice(4))
      } catch {
        return null
      }
    }
    return null
  }

  private load(): void {
    let raw: Partial<FileData>
    try {
      raw = JSON.parse(readFileSync(this.file, 'utf8')) as Partial<FileData>
    } catch (err) {
      // First launch: new file. Unreadable file: not overwritten.
      if ((err as NodeJS.ErrnoException).code !== 'ENOENT') this.blocked = true
      return
    }
    if (typeof raw.desktopId === 'string' && raw.desktopId) this.desktopId = raw.desktopId
    if (typeof raw.general === 'boolean') this.general = raw.general
    if (raw.defaultCert && typeof raw.defaultCert.certPem === 'string') {
      const key = this.open(raw.defaultCert.key)
      if (key) this.defaultCert = { certPem: raw.defaultCert.certPem, keyPem: key }
    }
    for (const l of Array.isArray(raw.links) ? raw.links : []) {
      if (!isLink(l)) continue
      const keyPem = this.open(l.key)
      if (!keyPem) continue // key lost (keychain changed): the link can no longer be used
      const { key: _key, ...rest } = l
      this.links.push({ ...rest, enabled: l.enabled !== false, lastSeen: l.lastSeen ?? null, peerPaused: l.peerPaused === true, keyPem })
    }
  }

  private serialize(): FileData {
    return {
      version: 1,
      desktopId: this.desktopId,
      general: this.general,
      ...(this.defaultCert && { defaultCert: { certPem: this.defaultCert.certPem, key: this.seal(this.defaultCert.keyPem) } }),
      links: this.links.map(({ keyPem, ...l }) => ({ ...l, key: this.seal(keyPem) }))
    }
  }

  /** Requests the write (asynchronous, atomic). */
  save(): void {
    this.writer.request()
  }

  /** Immediate write (app shutdown). */
  flush(): void {
    this.writer.flushSync()
  }

  get(linkId: string): StoredLink | undefined {
    return this.links.find((l) => l.linkId === linkId)
  }

  add(link: StoredLink): void {
    this.links.push(link)
    this.save()
  }

  remove(linkId: string): boolean {
    const i = this.links.findIndex((l) => l.linkId === linkId)
    if (i < 0) return false
    this.links.splice(i, 1)
    this.save()
    return true
  }
}

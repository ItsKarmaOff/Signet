// Link with computers, phone side: list of saved links, order of addresses to try, reconnection, choice of
// the computer for `localhost`. Pure functions (the transport is in app/mobile/src/features/link). Rules: ../link/README.md.
import { isCertSha256, isLinkId, isPort, type DevServer, type LinkDevice, type LinkErrorCode, type LinkPlatform } from '../link/protocol'
import { isLoopbackUrl, toLinkLocalhost } from '../link/loopback'
import { pickLocalhostRoute } from '../link/status'

/** An address where the computer can be reached. */
export interface HostPort {
  host: string
  port: number
}

/** Saved link. The token is never in it: it lives in the Keychain / Keystore. */
export interface StoredLink {
  linkId: string
  desktop: LinkDevice
  /** SHA-256 of the expected certificate (base64url). */
  certSha256: string
  /** Port and addresses from the QR code. */
  port: number
  hosts: string[]
  /** Last addresses that worked, most recent first. */
  last: HostPort[]
  /** Switch of this link on this phone. */
  enabled: boolean
  /** Last known "paused" state of the computer. */
  peerPaused: boolean
  /** The computer no longer knows this link. */
  invalid: boolean
  pairedAt: number
}

export const MAX_LAST_HOSTS = 4
export const MAX_LINKS = 16
/** Switch name and storage keys. */
export const LINK_TOKEN_KEY_PREFIX = 'signet.link.token.'
export const LINKS_STORAGE_KEY = 'signet.mobile.links'
export const PHONE_ID_KEY = 'signet.mobile.phoneId'

const PLATFORMS: readonly LinkPlatform[] = ['macos', 'windows', 'linux', 'ios', 'android']
const isObject = (v: unknown): v is Record<string, unknown> => !!v && typeof v === 'object' && !Array.isArray(v)

function parseHostPort(v: unknown): HostPort | null {
  return isObject(v) && typeof v.host === 'string' && v.host.length > 0 && v.host.length <= 64 && isPort(v.port) ? { host: v.host, port: v.port } : null
}

function parseStoredLink(v: unknown): StoredLink | null {
  if (!isObject(v) || !isLinkId(v.linkId) || !isCertSha256(v.certSha256) || !isPort(v.port) || !isObject(v.desktop)) return null
  const d = v.desktop
  if (typeof d.id !== 'string' || !d.id || typeof d.name !== 'string' || !d.name || !PLATFORMS.includes(d.platform as LinkPlatform)) return null
  return {
    linkId: v.linkId,
    desktop: { id: d.id, name: d.name.slice(0, 64), platform: d.platform as LinkPlatform },
    certSha256: v.certSha256,
    port: v.port,
    hosts: Array.isArray(v.hosts) ? v.hosts.filter((h): h is string => typeof h === 'string' && h.length > 0 && h.length <= 64).slice(0, 16) : [],
    last: Array.isArray(v.last)
      ? v.last
          .map(parseHostPort)
          .filter((h) => h !== null)
          .slice(0, MAX_LAST_HOSTS)
      : [],
    enabled: typeof v.enabled === 'boolean' ? v.enabled : true,
    peerPaused: v.peerPaused === true,
    invalid: v.invalid === true,
    pairedAt: typeof v.pairedAt === 'number' ? v.pairedAt : 0
  }
}

/** List read from storage: tolerant (malformed entries ignored, duplicate links removed). */
export function parseLinks(raw: unknown): StoredLink[] {
  if (!Array.isArray(raw)) return []
  const out: StoredLink[] = []
  for (const item of raw) {
    const l = parseStoredLink(item)
    if (l && !out.some((x) => x.linkId === l.linkId)) out.push(l)
  }
  return out.slice(0, MAX_LINKS)
}

export const serializeLinks = (links: readonly StoredLink[]): string => JSON.stringify(links)

/** Adds a link. A new pairing with the same computer replaces the old link (new keys). */
export function addLink(links: readonly StoredLink[], link: StoredLink): StoredLink[] {
  return [link, ...links.filter((l) => l.linkId !== link.linkId && l.desktop.id !== link.desktop.id)].slice(0, MAX_LINKS)
}

/** Links that a new pairing replaces (to delete their tokens). */
export const replacedLinks = (links: readonly StoredLink[], link: StoredLink): StoredLink[] =>
  links.filter((l) => l.linkId === link.linkId || l.desktop.id === link.desktop.id)

export const removeLink = (links: readonly StoredLink[], linkId: string): StoredLink[] => links.filter((l) => l.linkId !== linkId)

export const patchLink = (links: readonly StoredLink[], linkId: string, patch: Partial<Omit<StoredLink, 'linkId'>>): StoredLink[] =>
  links.map((l) => (l.linkId === linkId ? { ...l, ...patch } : l))

/** Remembers an address that worked, at the front (no duplicates, at most MAX_LAST_HOSTS). */
export function rememberEndpoint(last: readonly HostPort[], hp: HostPort): HostPort[] {
  return [hp, ...last.filter((x) => !(x.host === hp.host && x.port === hp.port))].slice(0, MAX_LAST_HOSTS)
}

export type EndpointSource = 'mdns' | 'last' | 'offer'
export interface Candidate extends HostPort {
  source: EndpointSource
}

/** Addresses to try, in order: seen over mDNS, then the last ones that worked, then those from the QR code. No duplicates. */
export function orderEndpoints(mdns: readonly HostPort[], link: Pick<StoredLink, 'last' | 'hosts' | 'port'>): Candidate[] {
  const out: Candidate[] = []
  const add = (hp: HostPort, source: EndpointSource) => {
    if (!out.some((x) => x.host === hp.host && x.port === hp.port)) out.push({ ...hp, source })
  }
  for (const hp of mdns) add(hp, 'mdns')
  for (const hp of link.last) add(hp, 'last')
  for (const host of link.hosts) add({ host, port: link.port }, 'offer')
  return out
}

/** Wait before reconnection number `attempt` (0, 1, 2…): 1 s, 2 s, 4 s… up to 30 s. */
export const backoffMs = (attempt: number): number => Math.min(30_000, 1000 * 2 ** Math.max(0, Math.min(attempt, 10)))

/** Interval of `ping`s and response timeout. */
export const PING_INTERVAL_MS = 20_000
export const PING_TIMEOUT_MS = 10_000

/** What the phone should do with a control connection given the wanted state: nothing, connect, pause then close, close. */
export type ControlAction = 'connect' | 'pause' | 'close' | 'none'
export function controlAction(s: {
  wantConnected: boolean
  hasConnection: boolean
  connected: boolean
  enabledHere: boolean
  invalid: boolean
}): ControlAction {
  if (s.invalid || !s.wantConnected) return s.hasConnection ? 'close' : 'none'
  // Switch off here: tell the computer before closing. Background closes silently.
  if (!s.enabledHere) return s.hasConnection ? (s.connected ? 'pause' : 'close') : 'none'
  return s.hasConnection ? 'none' : 'connect'
}

/** Pairing error message (translation key under `link.pairError`). */
export function pairErrorKey(code: LinkErrorCode | 'unreachable' | 'pin-mismatch' | 'timeout' | 'invalid' | 'version' | 'expired'): string {
  switch (code) {
    case 'expired':
      return 'expired'
    case 'bad-code':
    case 'unknown-link':
      return 'used'
    case 'version':
      return 'version'
    case 'invalid':
      return 'invalid'
    case 'pin-mismatch':
      return 'pin'
    default:
      return 'unreachable'
  }
}

// --- Choice of the computer for localhost ---

export interface ActiveLink {
  linkId: string
  connected: boolean
  peerPaused: boolean
}

/** Links usable now: connected, not paused on the computer, allowed here and valid. */
export function activeLinkIds(links: readonly StoredLink[], runtime: Readonly<Record<string, { connected: boolean } | undefined>>, general: boolean): string[] {
  if (!general) return []
  return links.filter((l) => l.enabled && !l.invalid && !l.peerPaused && runtime[l.linkId]?.connected).map((l) => l.linkId)
}

export type LocalhostDecision = { kind: 'none' } | { kind: 'route'; linkId: string; url: string } | { kind: 'ask'; url: string }

/** What to do with an address the user opens: outside localhost, nothing; otherwise map it back to `localhost` and route it, or ask which computer. */
export function decideLocalhost(url: string, active: readonly string[], chosen: string | null): LocalhostDecision {
  if (!isLoopbackUrl(url)) return { kind: 'none' }
  const pick = pickLocalhostRoute(active, chosen)
  if (pick === null) return { kind: 'none' }
  if (pick === 'ask') return { kind: 'ask', url: toLinkLocalhost(url) }
  return { kind: 'route', linkId: pick, url: toLinkLocalhost(url) }
}

/** Dev servers of a computer, sorted: open tabs first, then by port. */
export function sortServers(servers: readonly DevServer[]): DevServer[] {
  return [...servers].sort((a, b) => Number(!!b.fromTab) - Number(!!a.fromTab) || a.port - b.port)
}

/** Short label of a server: `localhost:5173`. */
export function serverLabel(s: DevServer): string {
  return `localhost:${s.port}`
}

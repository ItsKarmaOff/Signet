// Account-free link between devices: a phone paired once with a computer opens that computer's localhost
// when both are on the same network. This file defines what the two sides exchange: the QR code content and the messages.
//
// Transport (see also docs in app/mobile/modules/signet-link):
// - The computer listens on TCP + TLS (port LINK_DEFAULT_PORT if free) and announces itself over mDNS: `_signet._tcp`, TXT `v` and `id`.
// - Each link has its own certificate. The phone pins it (SHA-256 of the DER certificate, received in the QR code) and announces the link by
//   SNI (`serverNameForLink`). The computer picks the certificate from that name.
// - One connection = one role. The first line the phone sends is `pair` or `hello`. The computer answers with one line.
//   - `pair` (once only, with the QR code's code) → `paired` (with the link token) or `error`.
//   - `hello` role `control` → `welcome`, then control messages in both directions, one JSON line each.
//   - `hello` role `tunnel` + port → `ok`, then raw bytes in both directions to 127.0.0.1:port (or [::1]:port) on the computer.
// - Each message is a JSON object on one line ending with `\n`, at most LINK_MAX_LINE bytes.

import { base64UrlDecodeText, base64UrlEncodeText } from './base64url'
import { isLoopbackUrl } from './loopback'

export const LINK_PROTOCOL_VERSION = 1
/** Type de service mDNS (`_signet._tcp`). */
export const LINK_SERVICE_TYPE = 'signet'
/** Preferred computer port; if taken, a random free port (announced over mDNS and in the QR code). */
export const LINK_DEFAULT_PORT = 47615
/** Maximum line size (bytes, including `\n`). Beyond it, the connection is closed. */
export const LINK_MAX_LINE = 64 * 1024
/** Validity duration of a pairing QR code. */
export const PAIRING_TTL_MS = 5 * 60_000
/** Prefix of the link encoded in the QR code. The iOS camera also opens the app with this link. */
export const PAIRING_URL_PREFIX = 'signet://link?d='
/** Maximum number of ports relayed by the phone. */
export const MAX_PORTS = 256

export type LinkPlatform = 'macos' | 'windows' | 'linux' | 'ios' | 'android'
const PLATFORMS: readonly LinkPlatform[] = ['macos', 'windows', 'linux', 'ios', 'android']

/** A device: stable installation identifier, display name, platform. */
export interface LinkDevice {
  id: string
  name: string
  platform: LinkPlatform
}

/** QR code content shown by the computer. Single use, valid for PAIRING_TTL_MS. */
export interface PairingOffer {
  v: number
  /** Only the name travels in the QR code (confirmation screen); the identifier and platform arrive with `paired`. */
  desktop: Pick<LinkDevice, 'name'>
  /** Identifier of the link created by this pairing (16 lowercase hexadecimal characters). */
  linkId: string
  /** SHA-256 of this link's DER certificate, in base64url (43 characters). */
  certSha256: string
  /** Single-use code, base64url (22 characters = 16 bytes). */
  code: string
  /** IP addresses of the computer on the local network, at display time. */
  hosts: string[]
  port: number
  /** Expiry (ms since 1970). */
  expiresAt: number
}

/** Dev server offered to the phone: ports listening on the computer and open localhost tabs. */
export interface DevServer {
  /** Address to open on the phone, always on `localhost` (see `toLinkLocalhost`). */
  url: string
  port: number
  /** Title of the page (title tag) or of the tab, if known. */
  title?: string
  /** Comes from a tab open on the computer (otherwise from a listening port). */
  fromTab?: boolean
}

export type LinkErrorCode =
  | 'version' // unsupported protocol version
  | 'unknown-link' // the computer does not know (or no longer knows) this link
  | 'bad-token' // jeton faux
  | 'bad-code' // wrong or already used pairing code
  | 'expired' // expired QR code
  | 'paused' // link paused on the computer side (tunnel refused)
  | 'port-closed' // nothing listens on this computer port
  | 'port-refused' // forbidden port (the link's own)
  | 'bad-request' // malformed message

/** First line sent by the phone on a connection. */
export type ClientHello =
  | { type: 'pair'; v: number; linkId: string; code: string; device: LinkDevice }
  | { type: 'hello'; v: number; linkId: string; token: string; role: 'control'; device: LinkDevice }
  | { type: 'hello'; v: number; linkId: string; token: string; role: 'tunnel'; port: number }

/** The computer's reply to the first line. */
export type ServerReply =
  | { type: 'paired'; token: string; device: LinkDevice }
  | { type: 'welcome'; device: LinkDevice; paused: boolean }
  | { type: 'ok' }
  | { type: 'error'; code: LinkErrorCode }

/** Control messages, after `welcome`, in both directions unless noted otherwise. */
export type ControlMessage =
  /** The sender is paused (true) or active (false) for this link. */
  | { type: 'status'; paused: boolean }
  /**
   * Computer → phone: full list of dev servers (replaces the previous one). `servers`: the pages to offer;
   * `ports`: all ports listening on the computer (APIs included), which the phone relays from its own localhost.
   */
  | { type: 'servers'; servers: DevServer[]; ports: number[] }
  /** "Open on my phone" / "Open on the computer". */
  | { type: 'open'; url: string }
  /** The sender deleted the link: the other side deletes it too. */
  | { type: 'forget' }
  | { type: 'ping' }
  | { type: 'pong' }

export type LinkMessage = ClientHello | ServerReply | ControlMessage

// --- Identifiers and secrets (formats) ---

const LINK_ID = /^[0-9a-f]{16}$/
const B64URL = /^[A-Za-z0-9_-]+$/

export const isLinkId = (v: unknown): v is string => typeof v === 'string' && LINK_ID.test(v)
/** Link token: 32 bytes in base64url (43 characters). */
export const isLinkToken = (v: unknown): v is string => typeof v === 'string' && v.length === 43 && B64URL.test(v)
/** Pairing code: 16 bytes in base64url (22 characters). */
export const isPairingCode = (v: unknown): v is string => typeof v === 'string' && v.length === 22 && B64URL.test(v)
/** SHA-256 fingerprint in base64url (43 characters). */
export const isCertSha256 = (v: unknown): v is string => isLinkToken(v)
export const isPort = (v: unknown): v is number => typeof v === 'number' && Number.isInteger(v) && v > 0 && v < 65536

/** Name sent in SNI for a link. The computer derives the link identifier from it to pick the certificate. */
export function serverNameForLink(linkId: string): string {
  return `${linkId}.link.signet.invalid`
}

/** Inverse of `serverNameForLink`; null if the name does not have that shape. */
export function linkIdFromServerName(name: string | undefined): string | null {
  const m = /^([0-9a-f]{16})\.link\.signet\.invalid$/.exec(name ?? '')
  return m ? m[1]! : null
}

// --- QR code ---

/** Link to put in the QR code. */
export function encodePairingUrl(offer: PairingOffer): string {
  // Compact format, for a low-density QR code (readable by weaker cameras):
  // version~linkId~fingerprint~code~port~expiry(seconds, base 36)~addresses(commas)~name(base64url). `~` appears neither in base64url
  // nor in an IP address, and is still allowed in a URL: it separates the fields unambiguously.
  const fields = [
    String(offer.v),
    offer.linkId,
    offer.certSha256,
    offer.code,
    String(offer.port),
    Math.ceil(offer.expiresAt / 1000).toString(36),
    offer.hosts.join(','),
    base64UrlEncodeText(offer.desktop.name)
  ]
  return PAIRING_URL_PREFIX + fields.join('~')
}

export type PairingDecode = { ok: true; offer: PairingOffer } | { ok: false; error: 'invalid' | 'version' | 'expired' }

/** Reads the text of a QR code (or a `signet://link?d=…` link). `now` in ms. Also accepts the old format (JSON in base64url). */
export function decodePairingUrl(text: string, now: number): PairingDecode {
  const t = text.trim()
  if (!t.startsWith(PAIRING_URL_PREFIX)) return { ok: false, error: 'invalid' }
  const data = t.slice(PAIRING_URL_PREFIX.length)
  const raw = data.includes('~') ? compactFields(data) : legacyFields(data)
  if (!raw) return { ok: false, error: 'invalid' }
  if (raw.v !== LINK_PROTOCOL_VERSION) return { ok: false, error: typeof raw.v === 'number' && Number.isFinite(raw.v) ? 'version' : 'invalid' }
  const name = typeof raw.name === 'string' ? cleanName(raw.name) : ''
  const hosts = Array.isArray(raw.hosts) ? raw.hosts.filter(isHost).slice(0, 16) : null
  if (
    !name ||
    !hosts?.length ||
    !isLinkId(raw.linkId) ||
    !isCertSha256(raw.certSha256) ||
    !isPairingCode(raw.code) ||
    !isPort(raw.port) ||
    typeof raw.expiresAt !== 'number' ||
    !Number.isFinite(raw.expiresAt)
  )
    return { ok: false, error: 'invalid' }
  if (raw.expiresAt < now) return { ok: false, error: 'expired' }
  return {
    ok: true,
    offer: { v: raw.v, desktop: { name }, linkId: raw.linkId, certSha256: raw.certSha256, code: raw.code, hosts, port: raw.port, expiresAt: raw.expiresAt }
  }
}

interface RawOffer {
  v: unknown
  name: unknown
  linkId: unknown
  certSha256: unknown
  code: unknown
  hosts: unknown
  port: unknown
  expiresAt: unknown
}

function compactFields(data: string): RawOffer | null {
  const f = data.split('~')
  if (f.length !== 8) return null
  try {
    return {
      v: Number(f[0]),
      linkId: f[1],
      certSha256: f[2],
      code: f[3],
      port: Number(f[4]),
      expiresAt: parseInt(f[5]!, 36) * 1000,
      hosts: f[6]!.split(',').filter(Boolean),
      name: base64UrlDecodeText(f[7]!)
    }
  } catch {
    return null
  }
}

/** First format (full JSON in base64url), still accepted. */
function legacyFields(data: string): RawOffer | null {
  try {
    const raw: unknown = JSON.parse(base64UrlDecodeText(data))
    if (!isObject(raw)) return null
    return { ...raw, name: isObject(raw.desktop) ? raw.desktop.name : undefined } as RawOffer
  } catch {
    return null
  }
}

// --- Messages ---

/** A line ready to write (JSON + `\n`). */
export function encodeLinkMessage(msg: LinkMessage): string {
  return JSON.stringify(msg) + '\n'
}

/** Reads a received line (with or without `\n`). Null if malformed: the caller answers `bad-request` or closes. */
export function parseLinkMessage(line: string): LinkMessage | null {
  if (line.length > LINK_MAX_LINE) return null
  let m: unknown
  try {
    m = JSON.parse(line)
  } catch {
    return null
  }
  if (!isObject(m) || typeof m.type !== 'string') return null
  switch (m.type) {
    case 'pair': {
      const device = parseDevice(m.device)
      if (typeof m.v !== 'number' || !isLinkId(m.linkId) || !isPairingCode(m.code) || !device) return null
      return { type: 'pair', v: m.v, linkId: m.linkId, code: m.code, device }
    }
    case 'hello': {
      if (typeof m.v !== 'number' || !isLinkId(m.linkId) || !isLinkToken(m.token)) return null
      if (m.role === 'control') {
        const device = parseDevice(m.device)
        return device ? { type: 'hello', v: m.v, linkId: m.linkId, token: m.token, role: 'control', device } : null
      }
      if (m.role === 'tunnel') return isPort(m.port) ? { type: 'hello', v: m.v, linkId: m.linkId, token: m.token, role: 'tunnel', port: m.port } : null
      return null
    }
    case 'paired': {
      const device = parseDevice(m.device)
      return isLinkToken(m.token) && device ? { type: 'paired', token: m.token, device } : null
    }
    case 'welcome': {
      const device = parseDevice(m.device)
      return device && typeof m.paused === 'boolean' ? { type: 'welcome', device, paused: m.paused } : null
    }
    case 'ok':
      return { type: 'ok' }
    case 'error':
      return ERROR_CODES.includes(m.code as LinkErrorCode) ? { type: 'error', code: m.code as LinkErrorCode } : null
    case 'status':
      return typeof m.paused === 'boolean' ? { type: 'status', paused: m.paused } : null
    case 'servers': {
      if (!Array.isArray(m.servers)) return null
      const servers = m.servers
        .map(parseDevServer)
        .filter((s) => s !== null)
        .slice(0, 64)
      // Without `ports` (older version), at least relay the ports of the offered pages.
      const ports = Array.isArray(m.ports) ? m.ports.filter(isPort) : servers.map((s) => s.port)
      return { type: 'servers', servers, ports: [...new Set(ports)].sort((a, b) => a - b).slice(0, MAX_PORTS) }
    }
    case 'open':
      return typeof m.url === 'string' && m.url.length <= 4096 && /^https?:\/\//i.test(m.url) ? { type: 'open', url: m.url } : null
    case 'forget':
    case 'ping':
    case 'pong':
      return { type: m.type }
    default:
      return null
  }
}

const ERROR_CODES: readonly LinkErrorCode[] = [
  'version',
  'unknown-link',
  'bad-token',
  'bad-code',
  'expired',
  'paused',
  'port-closed',
  'port-refused',
  'bad-request'
]

function isObject(v: unknown): v is Record<string, unknown> {
  return typeof v === 'object' && v !== null && !Array.isArray(v)
}

function isHost(v: unknown): v is string {
  return typeof v === 'string' && v.length > 0 && v.length <= 64 && /^[0-9A-Za-z.:%-]+$/.test(v)
}

/** Device name: text, 1 to 64 characters, no control characters. */
function parseDevice(v: unknown): LinkDevice | null {
  if (!isObject(v) || typeof v.id !== 'string' || typeof v.name !== 'string' || !PLATFORMS.includes(v.platform as LinkPlatform)) return null
  const name = cleanName(v.name)
  if (!v.id || v.id.length > 64 || !name) return null
  return { id: v.id, name, platform: v.platform as LinkPlatform }
}

function parseDevServer(v: unknown): DevServer | null {
  if (!isObject(v) || typeof v.url !== 'string' || !isPort(v.port) || !isLoopbackUrl(v.url) || v.url.length > 2048) return null
  const s: DevServer = { url: v.url, port: v.port }
  if (typeof v.title === 'string' && v.title.trim()) s.title = v.title.trim().slice(0, 200)
  if (v.fromTab === true) s.fromTab = true
  return s
}

/** Displayable name: no control characters, 64 characters at most. */
function cleanName(name: string): string {
  return [...name]
    .filter((c) => c.charCodeAt(0) > 31 && c.charCodeAt(0) !== 127)
    .join('')
    .trim()
    .slice(0, 64)
}

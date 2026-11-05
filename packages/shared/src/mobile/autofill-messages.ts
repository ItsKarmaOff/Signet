// Messages between a page and the app (password manager and autofill), and the per-WebView session token that goes with them.
// Any frame of a page can call `ReactNativeWebView.postMessage`; only the main frame's injected scripts know the token, so a
// message without it (an iframe, a forged message) is dropped. Pure functions: the apps decide what to do with a valid message.

/** Name of the field carrying the token in every `af:*` and `pw:*` message. */
export const TOKEN_FIELD = 'k'

const MAX_ORIGIN = 2048
const MAX_FIELD = 1000
const MAX_STREET = 600
const MAX_NUMBER = 64

/** Random token for one WebView (32 hex characters). Uses `crypto.getRandomValues` when the engine has it. */
export function newPageToken(): string {
  const bytes = new Uint8Array(16)
  const c = (globalThis as { crypto?: { getRandomValues?: (a: Uint8Array) => Uint8Array } }).crypto
  if (c?.getRandomValues) c.getRandomValues(bytes)
  else for (let i = 0; i < bytes.length; i++) bytes[i] = Math.floor(Math.random() * 256)
  return Array.from(bytes, (b) => b.toString(16).padStart(2, '0')).join('')
}

const isPlain = (v: unknown): v is Record<string, unknown> => !!v && typeof v === 'object' && !Array.isArray(v)

/** Reads a WebView message (JSON text); `null` if it is not a JSON object. */
export function parseMessage(data: string): Record<string, unknown> | null {
  try {
    const v = JSON.parse(data) as unknown
    return isPlain(v) ? v : null
  } catch {
    return null
  }
}

/** Does the message carry this WebView's token? */
export const hasToken = (msg: unknown, token: string): boolean => !!token && isPlain(msg) && msg[TOKEN_FIELD] === token

export type AutofillMessageKind = 'address' | 'card'

export interface AddressFields {
  [field: string]: string
}

export interface CardFields {
  holder: string
  number: string
  exp: string
  expMonth: string
  expYear: string
}

export type AutofillMessage =
  | { type: 'af:focus'; kind: AutofillMessageKind; origin: string }
  | { type: 'af:blur' }
  | { type: 'af:submit'; origin: string; address?: AddressFields; card?: CardFields }
  | { type: 'af:filled'; ok: boolean }

const text = (v: unknown, max: number): string => (typeof v === 'string' ? v.slice(0, max) : '')

/** The origin of a message: an http(s) origin only. */
export function messageOrigin(v: unknown): string | null {
  if (typeof v !== 'string' || v.length > MAX_ORIGIN || !/^https?:\/\/[^/\s]+$/i.test(v)) return null
  return v
}

const ADDRESS_FIELDS = ['givenName', 'familyName', 'name', 'organization', 'street', 'city', 'postalCode', 'region', 'country', 'phone', 'email']

function addressFields(v: unknown): AddressFields | undefined {
  if (!isPlain(v)) return undefined
  const out: AddressFields = {}
  for (const k of ADDRESS_FIELDS) {
    const s = text(v[k], k === 'street' ? MAX_STREET : MAX_FIELD)
    if (s) out[k] = s
  }
  return Object.keys(out).length ? out : undefined
}

function cardFields(v: unknown): CardFields | undefined {
  if (!isPlain(v)) return undefined
  const card = {
    holder: text(v.holder, MAX_FIELD),
    number: text(v.number, MAX_NUMBER),
    exp: text(v.exp, 16),
    expMonth: text(v.expMonth, 8),
    expYear: text(v.expYear, 8)
  }
  return card.number ? card : undefined
}

/** Validates an `af:*` message: token, type, shape and field lengths. `null` = drop it. */
export function parseAutofillMessage(msg: unknown, token: string): AutofillMessage | null {
  if (!hasToken(msg, token) || !isPlain(msg)) return null
  switch (msg.type) {
    case 'af:focus': {
      const origin = messageOrigin(msg.origin)
      return origin && (msg.kind === 'address' || msg.kind === 'card') ? { type: 'af:focus', kind: msg.kind, origin } : null
    }
    case 'af:blur':
      return { type: 'af:blur' }
    case 'af:submit': {
      const origin = messageOrigin(msg.origin)
      if (!origin) return null
      const address = addressFields(msg.address)
      const card = cardFields(msg.card)
      return address || card ? { type: 'af:submit', origin, ...(address ? { address } : {}), ...(card ? { card } : {}) } : null
    }
    case 'af:filled':
      return typeof msg.ok === 'boolean' ? { type: 'af:filled', ok: msg.ok } : null
    default:
      return null
  }
}

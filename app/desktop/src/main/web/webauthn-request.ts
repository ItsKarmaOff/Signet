import { parse } from 'tldts'

// Validation of a WebAuthn request coming from a page, as a browser does it. Pure module (no Electron), tested.

export type WebAuthnErrorName = 'NotAllowedError' | 'SecurityError' | 'TypeError' | 'InvalidStateError'
export interface WebAuthnError {
  name: WebAuthnErrorName
  message: string
}

export interface WebAuthnRequest {
  type: 'get' | 'create'
  mediation?: string
  /** Effective relying party id (lowercase). */
  rpId: string
  /** Origin of the calling frame, from the main process (never from the page). */
  origin: string
  /** Options converted to plain data (BufferSource as base64url strings). */
  publicKey: Record<string, unknown>
}

export interface WebAuthnInput {
  origin: string | undefined
  topOrigin: string | undefined
  isMainFrame: boolean
  payload: unknown
}

export const NOT_ALLOWED_MESSAGE =
  'The operation either timed out or was not allowed. See: https://www.w3.org/TR/webauthn-2/#sctn-privacy-considerations-client.'
const INSECURE_MESSAGE = 'The operation is insecure.'
const RPID_MESSAGE = 'The relying party ID is not a registrable domain suffix of, nor equal to the current domain.'

const notAllowed = (message = NOT_ALLOWED_MESSAGE): WebAuthnError => ({ name: 'NotAllowedError', message })
const insecure = (message = INSECURE_MESSAGE): WebAuthnError => ({ name: 'SecurityError', message })
const typeError = (message: string): WebAuthnError => ({ name: 'TypeError', message })

const isObject = (v: unknown): v is Record<string, unknown> => typeof v === 'object' && v !== null && !Array.isArray(v)

const isLocalhost = (host: string): boolean => host === 'localhost' || host.endsWith('.localhost')
const isIpHost = (host: string): boolean => host.startsWith('[') || host.includes(':') || /^\d+(\.\d+){3}$/.test(host)

/** Checks the payload's shape (reasonable, not the whole spec); returns an error message or null. */
function malformed(type: string, publicKey: Record<string, unknown>): string | null {
  if (typeof publicKey.challenge !== 'string') return "Failed to read the 'challenge' property from the options: not a BufferSource."
  if (type === 'create') {
    if (!isObject(publicKey.rp)) return "Failed to read the 'rp' property from the options: not an object."
    if (!isObject(publicKey.user) || typeof publicKey.user.id !== 'string') return "Failed to read the 'user' property from the options: missing 'id'."
    if (!Array.isArray(publicKey.pubKeyCredParams)) return "Failed to read the 'pubKeyCredParams' property from the options: not a sequence."
  }
  return null
}

/** True if `rpId` is the host or a parent domain of it (label boundary) that is not a public suffix. */
function rpIdAllowed(host: string, rpId: string): boolean {
  if (rpId === host) return !isIpHost(host) && (isLocalhost(host) || !!parse(host, { allowPrivateDomains: true }).domain)
  if (!host.endsWith('.' + rpId)) return false
  if (rpId === 'localhost') return true
  return !!parse(rpId, { allowPrivateDomains: true }).domain
}

/** Validates a request like a browser (secure origin, rpId, frame, shape): the validated request, or the error to answer. */
export function validateWebAuthnRequest(input: WebAuthnInput): WebAuthnRequest | WebAuthnError {
  const { origin, topOrigin, isMainFrame, payload } = input
  if (!origin || origin === 'null') return notAllowed()
  let url: URL
  try {
    url = new URL(origin)
  } catch {
    return notAllowed()
  }
  if (!isObject(payload)) return typeError('The WebAuthn request is malformed.')
  const { type, mediation, publicKey } = payload
  if (type !== 'get' && type !== 'create') return typeError('The WebAuthn request type is unknown.')
  if (!isObject(publicKey)) return typeError("Failed to read the 'publicKey' property from the options: not an object.")
  const bad = malformed(type, publicKey)
  if (bad) return typeError(bad)

  const host = url.hostname.toLowerCase()
  const secure = url.protocol === 'https:' || (url.protocol === 'http:' && isLocalhost(host))
  if (!secure || isIpHost(host)) return insecure()

  let rpId = host
  if (type === 'get') {
    if (publicKey.rpId !== undefined) rpId = publicKey.rpId as string
  } else if (publicKey.rp && (publicKey.rp as Record<string, unknown>).id !== undefined) {
    rpId = (publicKey.rp as Record<string, unknown>).id as string
  }
  if (typeof rpId !== 'string') return typeError("The 'rpId' property is not a string.")
  rpId = rpId.toLowerCase()
  if (!rpId || !rpIdAllowed(host, rpId)) return insecure(RPID_MESSAGE)

  if (!isMainFrame && origin !== topOrigin) return notAllowed()

  return { type, mediation: typeof mediation === 'string' ? mediation : undefined, rpId, origin, publicKey }
}

export const isWebAuthnError = (r: WebAuthnRequest | WebAuthnError): r is WebAuthnError => 'message' in r

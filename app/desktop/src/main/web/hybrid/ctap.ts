import { createHash, createPublicKey } from 'node:crypto'
import { NOT_ALLOWED_MESSAGE, type WebAuthnError, type WebAuthnRequest } from '../webauthn-request'
import { decodeCbor, decodeCborPrefix, encodeCbor, type CborValue } from './cbor'

// CTAP2 side of a passkey request answered by a phone: builds authenticatorMakeCredential / authenticatorGetAssertion from
// the WebAuthn options like Chromium does, then checks the response and turns it into a credential (plain data, base64url,
// the shape of WebAuthn's toJSON()). Pure module (no Electron), tested. Checked against Chromium, base:
// https://raw.githubusercontent.com/chromium/chromium/main/
//   device/fido/ctap_make_credential_request.cc, ctap_get_assertion_request.cc (integer keys, options rk / uv / up)
//   device/fido/public/public_key_credential_{rp_entity,user_entity,params,descriptor}.cc (field names, transports omitted)
//   device/fido/fido_device_authenticator.cc (uv from getInfo, empty displayName kept over hybrid)
//   device/fido/make_credential_request_handler.cc (rk required / preferred, uv upgrade, rpIdHash check)
//   device/fido/get_assertion_request_handler.cc (ResponseValid), device/fido/device_response_converter.cc (response keys)
//   device/fido/authenticator_data.cc, attested_credential_data.cc, attestation_object.cc (parsing, "none" attestation)
//   components/webauthn/core/browser/client_data_json.cc (clientDataJSON byte layout and escaping)
//   content/browser/webauth/authenticator_common_impl.cc (attestation erasure for hybrid: statement erased, AAGUID kept;
//     transports ['hybrid', 'internal'] by default; credProps; CREDENTIAL_EXCLUDED -> InvalidStateError)
// Contents of requests and responses are never logged.

const CMD_MAKE_CREDENTIAL = 0x01
const CMD_GET_ASSERTION = 0x02
const STATUS_SUCCESS = 0x00
const STATUS_CREDENTIAL_EXCLUDED = 0x19

const FLAG_UP = 0x01
const FLAG_AT = 0x40
const FLAG_ED = 0x80
const AUTH_DATA_MIN = 37
const MAX_CREDENTIAL_ID = 1023

const ALG_ES256 = -7
const ALG_EDDSA = -8
const ALG_RS256 = -257

const EXCLUDED_MESSAGE = 'The user attempted to register an authenticator that contains one of the credentials already registered with the relying party.'

/** A failure to answer with: `error` goes to the page, `reason` is a log line (no data). */
export class CtapFailure extends Error {
  constructor(
    readonly error: WebAuthnError,
    readonly reason: string
  ) {
    super(reason)
    this.name = 'CtapFailure'
  }
}

const notAllowed = (reason: string): CtapFailure => new CtapFailure({ name: 'NotAllowedError', message: NOT_ALLOWED_MESSAGE }, reason)

/** Chrome's mapping of a CTAP status to a DOMException: excluded credential -> InvalidStateError, everything else NotAllowedError. */
export function mapCtapStatus(status: number): WebAuthnError {
  if (status === STATUS_CREDENTIAL_EXCLUDED) return { name: 'InvalidStateError', message: EXCLUDED_MESSAGE }
  return { name: 'NotAllowedError', message: NOT_ALLOWED_MESSAGE }
}

/** The credential as the page gets it, in toJSON() form (binary fields are base64url). */
export interface CredentialJson {
  id: string
  rawId: string
  type: 'public-key'
  authenticatorAttachment: 'cross-platform'
  response: AssertionResponseJson | AttestationResponseJson
  clientExtensionResults: { credProps?: { rk?: boolean } }
}
export interface AssertionResponseJson {
  clientDataJSON: string
  authenticatorData: string
  signature: string
  userHandle?: string
}
export interface AttestationResponseJson {
  clientDataJSON: string
  attestationObject: string
  authenticatorData: string
  transports: string[]
  publicKey?: string
  publicKeyAlgorithm?: number
}

const b64 = (b: Uint8Array): string => Buffer.from(b).toString('base64url')

/** JSON string escaping of clientDataJSON: only the quote, the backslash and control characters (lowercase \u00xx). */
function jsonString(s: string): string {
  let out = '"'
  for (const ch of s) {
    const c = ch.codePointAt(0)!
    if (c === 0x22) out += '\\"'
    else if (c === 0x5c) out += '\\\\'
    else if (c < 0x20) out += '\\u00' + c.toString(16).padStart(2, '0')
    else out += ch
  }
  return out + '"'
}

/** CollectedClientData exactly as Chrome serializes it for a same-origin frame (without the random extra key it adds sometimes). */
export function clientDataJson(type: 'get' | 'create', challenge: Uint8Array, origin: string): string {
  return `{"type":"webauthn.${type}","challenge":${jsonString(b64(challenge))},"origin":${jsonString(origin)},"crossOrigin":false}`
}

/** What the request needs to know about the phone, from its getInfo response. */
export interface DeviceInfo {
  versions: string[]
  /** options.uv === true: user verification is configured. */
  uv: boolean
  rk: boolean
  makeCredUvNotRqd: boolean
  transports: string[] | undefined
}

export function parseGetInfo(bytes: Uint8Array): DeviceInfo {
  let map: CborValue
  try {
    map = decodeCbor(bytes)
  } catch {
    throw notAllowed('invalid getInfo')
  }
  if (!(map instanceof Map)) throw notAllowed('invalid getInfo')
  const strings = (v: CborValue): string[] | undefined => (Array.isArray(v) && v.every((x) => typeof x === 'string') ? (v as string[]) : undefined)
  const options = map.get(4)
  const flag = (name: string): boolean => options instanceof Map && options.get(name) === true
  return {
    versions: strings(map.get(1)) ?? [],
    uv: flag('uv'),
    rk: flag('rk'),
    makeCredUvNotRqd: flag('makeCredUvNotRqd'),
    transports: strings(map.get(9))
  }
}

const isObject = (v: unknown): v is Record<string, unknown> => typeof v === 'object' && v !== null && !Array.isArray(v)

function bytesOf(v: unknown, what: string): Buffer {
  if (typeof v !== 'string') throw notAllowed(`${what} is not a buffer`)
  return Buffer.from(v, 'base64url')
}

/** Credential descriptors of type public-key (others are ignored like Blink does); transports are not sent. */
function descriptors(list: unknown): Buffer[] {
  if (list === undefined) return []
  if (!Array.isArray(list)) throw notAllowed('credential list is not a sequence')
  const ids: Buffer[] = []
  for (const d of list) {
    if (!isObject(d)) throw notAllowed('invalid credential descriptor')
    if (d.type === 'public-key') ids.push(bytesOf(d.id, 'credential id'))
  }
  return ids
}

const descriptor = (id: Buffer): CborValue =>
  new Map<CborValue, CborValue>([
    ['id', new Uint8Array(id)],
    ['type', 'public-key']
  ])

/** A request ready to send, with what is needed to check the answer. */
export interface CtapPlan {
  type: 'get' | 'create'
  /** Command byte + CBOR. */
  bytes: Buffer
  clientDataJSON: Buffer
  rpId: string
  allowIds: Buffer[]
  attestation: string
  /** rk sent to the phone. */
  residentKey: boolean
  credProps: boolean
  info: DeviceInfo
  /** Names of requested extensions that are not passed to the phone (development log). */
  skipped: string[]
}

function pubKeyCredParams(raw: unknown): CborValue[] {
  if (!Array.isArray(raw)) throw notAllowed('pubKeyCredParams is not a sequence')
  const params = raw
    .filter((p): p is Record<string, unknown> => isObject(p) && p.type === 'public-key' && typeof p.alg === 'number' && Number.isInteger(p.alg))
    .map(
      (p) =>
        new Map<CborValue, CborValue>([
          ['alg', p.alg as number],
          ['type', 'public-key']
        ])
    )
  // An empty list means the defaults (ES256, RS256); a list of unknown types leaves nothing to create.
  if (raw.length === 0)
    return [ALG_ES256, ALG_RS256].map(
      (alg) =>
        new Map<CborValue, CborValue>([
          ['alg', alg],
          ['type', 'public-key']
        ])
    )
  if (params.length === 0) throw notAllowed('no supported credential type')
  return params
}

/** Builds the CTAP2 command for a request, once the phone's getInfo is known. */
export function planCtap(request: WebAuthnRequest, info: DeviceInfo): CtapPlan {
  const pk = request.publicKey
  const challenge = bytesOf(pk.challenge, 'challenge')
  const json = Buffer.from(clientDataJson(request.type, challenge, request.origin), 'utf8')
  const hash = createHash('sha256').update(json).digest()
  const extensions = isObject(pk.extensions) ? pk.extensions : {}
  const skipped = Object.keys(extensions).filter((k) => k !== 'credProps')
  const plan = { type: request.type, clientDataJSON: json, rpId: request.rpId, attestation: 'none', residentKey: false, credProps: false, info, skipped }

  if (request.type === 'get') {
    const allowIds = descriptors(pk.allowCredentials)
    // Resident credential requests on non-platform authenticators always require UV.
    const wanted = allowIds.length === 0 ? 'required' : typeof pk.userVerification === 'string' ? pk.userVerification : 'preferred'
    if (wanted === 'required' && !info.uv) throw notAllowed('user verification required but not available')
    const map = new Map<CborValue, CborValue>([
      [1, request.rpId],
      [2, new Uint8Array(hash)]
    ])
    if (allowIds.length) map.set(3, allowIds.map(descriptor))
    if (info.uv && wanted !== 'discouraged') map.set(5, new Map<CborValue, CborValue>([['uv', true]]))
    return { ...plan, bytes: Buffer.concat([Buffer.of(CMD_GET_ASSERTION), encodeCbor(map)]), allowIds }
  }

  const rp = pk.rp as Record<string, unknown>
  const user = pk.user as Record<string, unknown>
  const selection = isObject(pk.authenticatorSelection) ? pk.authenticatorSelection : {}
  const residentKey = typeof selection.residentKey === 'string' ? selection.residentKey : selection.requireResidentKey === true ? 'required' : 'discouraged'
  const rk = residentKey === 'required' || (residentKey === 'preferred' && info.rk && info.uv)
  if (residentKey === 'required' && !info.rk) throw notAllowed('resident keys required but not supported')
  const wanted = rk ? 'required' : typeof selection.userVerification === 'string' ? selection.userVerification : 'preferred'
  if (wanted === 'required' && !info.uv) throw notAllowed('user verification required but not available')

  const rpMap = new Map<CborValue, CborValue>([['id', request.rpId]])
  if (typeof rp.name === 'string') rpMap.set('name', rp.name)
  const userMap = new Map<CborValue, CborValue>([
    ['id', new Uint8Array(bytesOf(user.id, 'user id'))],
    // Phones refuse a request without a displayName, even an empty one (see FidoDeviceAuthenticator::MakeCredential).
    ['displayName', typeof user.displayName === 'string' ? user.displayName : '']
  ])
  if (typeof user.name === 'string') userMap.set('name', user.name)
  const map = new Map<CborValue, CborValue>([
    [1, new Uint8Array(hash)],
    [2, rpMap],
    [3, userMap],
    [4, pubKeyCredParams(pk.pubKeyCredParams)]
  ])
  const excludeIds = descriptors(pk.excludeCredentials)
  if (excludeIds.length) map.set(5, excludeIds.map(descriptor))
  const options = new Map<CborValue, CborValue>()
  if (rk) options.set('rk', true)
  // UV configured on the phone is always used for a registration, unless it says it is not needed.
  if (info.uv && !info.makeCredUvNotRqd) options.set('uv', true)
  if (options.size) map.set(7, options)
  return {
    ...plan,
    bytes: Buffer.concat([Buffer.of(CMD_MAKE_CREDENTIAL), encodeCbor(map)]),
    allowIds: [],
    attestation: typeof pk.attestation === 'string' ? pk.attestation : 'none',
    residentKey: rk,
    credProps: extensions.credProps === true
  }
}

interface AuthData {
  rpIdHash: Buffer
  flags: number
  attested?: { credentialId: Buffer; publicKey: Map<CborValue, CborValue> }
}

/** Authenticator data: rpIdHash, flags, counter, optional attested credential data, optional extensions (as Chromium parses it). */
function parseAuthData(bytes: Uint8Array): AuthData {
  if (bytes.length < AUTH_DATA_MIN) throw notAllowed('authenticator data too short')
  const buf = Buffer.from(bytes)
  const flags = buf[32]!
  let at = AUTH_DATA_MIN
  let attested: AuthData['attested']
  if (flags & FLAG_AT) {
    if (buf.length < at + 18) throw notAllowed('truncated attested credential data')
    const idLength = buf.readUInt16BE(at + 16)
    at += 18
    if (idLength === 0 || idLength > MAX_CREDENTIAL_ID || buf.length < at + idLength) throw notAllowed('invalid credential id')
    const credentialId = buf.subarray(at, at + idLength)
    at += idLength
    let key: { value: CborValue; length: number }
    try {
      key = decodeCborPrefix(buf.subarray(at))
    } catch {
      throw notAllowed('invalid public key')
    }
    if (!(key.value instanceof Map)) throw notAllowed('invalid public key')
    at += key.length
    attested = { credentialId, publicKey: key.value }
  }
  if (flags & FLAG_ED) {
    let ext: CborValue
    try {
      ext = decodeCbor(buf.subarray(at))
    } catch {
      throw notAllowed('invalid extensions')
    }
    if (!(ext instanceof Map)) throw notAllowed('invalid extensions')
  } else if (at !== buf.length) {
    throw notAllowed('trailing authenticator data')
  }
  return { rpIdHash: buf.subarray(0, 32), flags, attested }
}

/** SPKI DER of a COSE public key (ES256, EdDSA, RS256); undefined for anything else, like Chrome. */
function spkiOf(key: Map<CborValue, CborValue>): { der?: Buffer; alg: number } {
  const alg = key.get(3)
  if (typeof alg !== 'number') throw notAllowed('invalid public key algorithm')
  const bin = (k: number): string | undefined => {
    const v = key.get(k)
    return v instanceof Uint8Array ? Buffer.from(v).toString('base64url') : undefined
  }
  let jwk: Record<string, string | undefined> | null = null
  if (alg === ALG_ES256 && key.get(1) === 2 && key.get(-1) === 1) jwk = { kty: 'EC', crv: 'P-256', x: bin(-2), y: bin(-3) }
  else if (alg === ALG_EDDSA && key.get(1) === 1 && key.get(-1) === 6) jwk = { kty: 'OKP', crv: 'Ed25519', x: bin(-2) }
  else if (alg === ALG_RS256 && key.get(1) === 3) jwk = { kty: 'RSA', n: bin(-1), e: bin(-2) }
  if (!jwk) return { alg }
  try {
    return { alg, der: createPublicKey({ key: jwk as never, format: 'jwk' }).export({ type: 'spki', format: 'der' }) }
  } catch {
    return { alg }
  }
}

const KNOWN_TRANSPORTS = ['ble', 'hybrid', 'internal', 'nfc', 'usb']

/** Transports as Chrome reports them: the phone's own list plus hybrid, ['hybrid', 'internal'] when it gives none; sorted. */
function transportsOf(info: DeviceInfo): string[] {
  const set = new Set(info.transports ? info.transports.filter((t) => KNOWN_TRANSPORTS.includes(t)) : ['internal'])
  set.add('hybrid')
  return [...set].sort()
}

function checkAuthData(plan: CtapPlan, auth: AuthData): void {
  if (!auth.rpIdHash.equals(createHash('sha256').update(plan.rpId).digest())) throw notAllowed('rpIdHash mismatch')
  if (!(auth.flags & FLAG_UP)) throw notAllowed('user presence flag missing')
}

/**
 * Checks the phone's answer to `plan` and builds the credential. Throws CtapFailure (NotAllowedError, or InvalidStateError
 * for an excluded credential).
 */
export function parseCtapResponse(plan: CtapPlan, response: Uint8Array): CredentialJson {
  if (response.length === 0) throw notAllowed('empty CTAP response')
  const status = response[0]!
  if (status !== STATUS_SUCCESS) throw new CtapFailure(mapCtapStatus(status), `CTAP status 0x${status.toString(16).padStart(2, '0')}`)
  let map: CborValue
  try {
    map = decodeCbor(response.subarray(1))
  } catch {
    throw notAllowed('invalid CBOR in CTAP response')
  }
  if (!(map instanceof Map)) throw notAllowed('CTAP response is not a map')
  return plan.type === 'get' ? parseAssertion(plan, map) : parseAttestation(plan, map)
}

function parseAssertion(plan: CtapPlan, map: Map<CborValue, CborValue>): CredentialJson {
  const authBytes = map.get(2)
  const signature = map.get(3)
  if (!(authBytes instanceof Uint8Array) || !(signature instanceof Uint8Array)) throw notAllowed('missing assertion fields')
  const auth = parseAuthData(authBytes)
  checkAuthData(plan, auth)
  if (auth.attested) throw notAllowed('unexpected attested credential data')

  // The credential is left out when the allow list has a single entry.
  let id: Buffer
  const credential = map.get(1)
  if (credential instanceof Map) {
    const raw = credential.get('id')
    if (!(raw instanceof Uint8Array) || raw.length === 0 || raw.length > MAX_CREDENTIAL_ID) throw notAllowed('invalid credential')
    id = Buffer.from(raw)
  } else if (credential === undefined && plan.allowIds.length === 1) id = plan.allowIds[0]!
  else throw notAllowed('missing credential')
  if (plan.allowIds.length && !plan.allowIds.some((a) => a.equals(id))) throw notAllowed('credential not in the allow list')

  const user = map.get(4)
  let userHandle: Uint8Array | undefined
  if (user !== undefined) {
    const uid = user instanceof Map ? user.get('id') : undefined
    if (!(uid instanceof Uint8Array)) throw notAllowed('invalid user entity')
    userHandle = uid
  }
  if (plan.allowIds.length === 0 && !userHandle) throw notAllowed('user entity missing')

  const response: AssertionResponseJson = {
    clientDataJSON: b64(plan.clientDataJSON),
    authenticatorData: b64(authBytes),
    signature: b64(signature)
  }
  if (userHandle && userHandle.length) response.userHandle = b64(userHandle)
  return { id: b64(id), rawId: b64(id), type: 'public-key', authenticatorAttachment: 'cross-platform', response, clientExtensionResults: {} }
}

function parseAttestation(plan: CtapPlan, map: Map<CborValue, CborValue>): CredentialJson {
  const fmt = map.get(1)
  const authBytes = map.get(2)
  const attStmt = map.get(3)
  if (typeof fmt !== 'string' || !(authBytes instanceof Uint8Array) || !(attStmt instanceof Map)) throw notAllowed('missing attestation fields')
  const auth = parseAuthData(authBytes)
  checkAuthData(plan, auth)
  if (!auth.attested) throw notAllowed('no attested credential data')

  // "none" (the default): the statement is erased, the AAGUID kept (hybrid authenticators are treated like platform ones).
  const none = plan.attestation === 'none'
  const attestationObject = encodeCbor(
    new Map<CborValue, CborValue>([
      ['fmt', none ? 'none' : fmt],
      ['attStmt', none ? new Map<CborValue, CborValue>() : attStmt],
      ['authData', authBytes]
    ])
  )
  const id = auth.attested.credentialId
  const key = spkiOf(auth.attested.publicKey)
  const response: AttestationResponseJson = {
    clientDataJSON: b64(plan.clientDataJSON),
    attestationObject: b64(attestationObject),
    authenticatorData: b64(authBytes),
    transports: transportsOf(plan.info)
  }
  if (key.der) response.publicKey = b64(key.der)
  response.publicKeyAlgorithm = key.alg

  const results: CredentialJson['clientExtensionResults'] = {}
  if (plan.credProps) {
    // Only known when a resident key was required; CTAP 2.1 (or no rk support) means "not resident", an old phone says nothing.
    const newer = plan.info.versions.some((v) => v === 'FIDO_2_1' || v === 'FIDO_2_2')
    results.credProps = plan.residentKey ? { rk: true } : !plan.info.rk || newer ? { rk: false } : {}
  }
  return { id: b64(id), rawId: b64(id), type: 'public-key', authenticatorAttachment: 'cross-platform', response, clientExtensionResults: results }
}

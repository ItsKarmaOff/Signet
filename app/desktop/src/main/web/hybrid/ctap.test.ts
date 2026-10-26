import { createHash, generateKeyPairSync } from 'node:crypto'
import { describe, expect, it } from 'vitest'
import type { WebAuthnRequest } from '../webauthn-request'
import { decodeCbor, encodeCbor, type CborValue } from './cbor'
import { clientDataJson, CtapFailure, mapCtapStatus, parseCtapResponse, parseGetInfo, planCtap, type CtapPlan, type DeviceInfo } from './ctap'

const sha = (s: string | Uint8Array): Buffer => createHash('sha256').update(s).digest()
const b64 = (b: Uint8Array): string => Buffer.from(b).toString('base64url')
const map = (entries: [CborValue, CborValue][]): Map<CborValue, CborValue> => new Map(entries)

const challenge = Uint8Array.from({ length: 32 }, (_, i) => i)
const phone: DeviceInfo = { versions: ['U2F_V2', 'FIDO_2_0'], uv: true, rk: true, makeCredUvNotRqd: false, transports: undefined }

const getRequest = (publicKey: Record<string, unknown> = {}, rpId = 'login.microsoftonline.com'): WebAuthnRequest => ({
  type: 'get',
  rpId,
  origin: `https://${rpId}`,
  publicKey: { challenge: b64(challenge), rpId, userVerification: 'preferred', allowCredentials: [], timeout: 60000, ...publicKey }
})

const userId = Uint8Array.of(9, 8, 7)
const createRequest = (publicKey: Record<string, unknown> = {}): WebAuthnRequest => ({
  type: 'create',
  rpId: 'webauthn.io',
  origin: 'https://webauthn.io',
  publicKey: {
    challenge: b64(challenge),
    rp: { name: 'webauthn.io', id: 'webauthn.io' },
    user: { id: b64(userId), name: 'alice', displayName: 'Alice' },
    pubKeyCredParams: [
      { type: 'public-key', alg: -7 },
      { type: 'public-key', alg: -257 }
    ],
    authenticatorSelection: { residentKey: 'preferred', userVerification: 'preferred' },
    attestation: 'none',
    extensions: { credProps: true },
    ...publicKey
  }
})

/** The request map of a plan (the command byte apart). */
const decoded = (plan: CtapPlan): Map<CborValue, CborValue> => decodeCbor(plan.bytes.subarray(1)) as Map<CborValue, CborValue>

describe('clientDataJson', () => {
  it('is byte-exact like Chrome (type, challenge, origin, crossOrigin)', () => {
    expect(clientDataJson('get', Uint8Array.of(1, 2, 3), 'https://example.com')).toBe(
      '{"type":"webauthn.get","challenge":"AQID","origin":"https://example.com","crossOrigin":false}'
    )
    expect(clientDataJson('create', challenge, 'https://webauthn.io:8443')).toBe(
      '{"type":"webauthn.create","challenge":"AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8","origin":"https://webauthn.io:8443","crossOrigin":false}'
    )
  })

  it('escapes only quotes, backslashes and control characters (lowercase hex)', () => {
    const json = clientDataJson('get', Uint8Array.of(), 'x"\\\n\u001f/é<')
    expect(json).toContain('"origin":"x\\"\\\\\\u000a\\u001f/é<"')
    expect(() => JSON.parse(json)).not.toThrow()
  })
})

describe('parseGetInfo', () => {
  it('reads versions, uv, rk, makeCredUvNotRqd and transports', () => {
    const info = parseGetInfo(
      encodeCbor(
        map([
          [1, ['FIDO_2_0', 'FIDO_2_1']],
          [
            4,
            map([
              ['uv', true],
              ['rk', true],
              ['makeCredUvNotRqd', true],
              ['up', true]
            ])
          ],
          [9, ['internal', 'hybrid']]
        ])
      )
    )
    expect(info).toEqual({ versions: ['FIDO_2_0', 'FIDO_2_1'], uv: true, rk: true, makeCredUvNotRqd: true, transports: ['internal', 'hybrid'] })
  })

  it('an empty map means nothing is supported', () => {
    expect(parseGetInfo(Uint8Array.of(0xa0))).toEqual({ versions: [], uv: false, rk: false, makeCredUvNotRqd: false, transports: undefined })
  })

  it('refuses what is not a CBOR map', () => {
    expect(() => parseGetInfo(Uint8Array.of(0x01))).toThrow(CtapFailure)
    expect(() => parseGetInfo(Uint8Array.of(0xff))).toThrow(CtapFailure)
  })
})

describe('planCtap: getAssertion', () => {
  it('Microsoft-like request (no allowed credentials): rpId, clientDataHash, uv', () => {
    const plan = planCtap(getRequest(), phone)
    expect(plan.bytes[0]).toBe(0x02)
    const json = clientDataJson('get', challenge, 'https://login.microsoftonline.com')
    expect(plan.clientDataJSON.toString()).toBe(json)
    expect(decoded(plan)).toEqual(
      map([
        [1, 'login.microsoftonline.com'],
        [2, new Uint8Array(sha(json))],
        [5, map([['uv', true]])]
      ])
    )
    // Canonical bytes: a3 01 <rpId> 02 5820 <hash> 05 a1 62 uv f5
    const hex = plan.bytes.toString('hex')
    expect(
      hex.startsWith(
        '02a301' + '78' + '19' + Buffer.from('login.microsoftonline.com').toString('hex') + '025820' + sha(json).toString('hex') + '05a162' + '7576' + 'f5'
      )
    ).toBe(true)
  })

  it('sends the allow list as descriptors without transports', () => {
    const plan = planCtap(
      getRequest({
        allowCredentials: [
          { type: 'public-key', id: 'AQID', transports: ['internal'] },
          { type: 'other', id: 'BAUG' },
          { type: 'public-key', id: 'BwgJ' }
        ]
      }),
      phone
    )
    expect(decoded(plan).get(3)).toEqual([
      map([
        ['id', Uint8Array.of(1, 2, 3)],
        ['type', 'public-key']
      ]),
      map([
        ['id', Uint8Array.of(7, 8, 9)],
        ['type', 'public-key']
      ])
    ])
    // Descriptor keys in canonical order: "id" before "type".
    expect(plan.bytes.toString('hex')).toContain('a2' + '626964' + '43010203' + '6474797065' + '6a7075626c69632d6b6579')
  })

  it('uv follows the phone: not sent when not configured or when discouraged', () => {
    expect(decoded(planCtap(getRequest({ allowCredentials: [{ type: 'public-key', id: 'AQID' }], userVerification: 'discouraged' }), phone)).has(5)).toBe(false)
    expect(decoded(planCtap(getRequest({ allowCredentials: [{ type: 'public-key', id: 'AQID' }] }), { ...phone, uv: false })).has(5)).toBe(false)
    // No allowed credentials: user verification is required whatever the page says.
    expect(decoded(planCtap(getRequest({ userVerification: 'discouraged' }), phone)).get(5)).toEqual(map([['uv', true]]))
  })

  it('fails when user verification is required but the phone cannot do it', () => {
    expect(() =>
      planCtap(getRequest({ userVerification: 'required', allowCredentials: [{ type: 'public-key', id: 'AQID' }] }), { ...phone, uv: false })
    ).toThrow(CtapFailure)
    expect(() => planCtap(getRequest(), { ...phone, uv: false })).toThrow(CtapFailure)
  })

  it('lists extensions that are not passed on', () => {
    expect(planCtap(getRequest({ extensions: { prf: {}, appid: 'x', largeBlob: {} } }), phone).skipped).toEqual(['prf', 'appid', 'largeBlob'])
  })
})

describe('planCtap: makeCredential', () => {
  it('webauthn.io-like request: rp, user (with displayName), params, options rk and uv', () => {
    const plan = planCtap(createRequest(), phone)
    expect(plan.bytes[0]).toBe(0x01)
    const json = clientDataJson('create', challenge, 'https://webauthn.io')
    expect(decoded(plan)).toEqual(
      map([
        [1, new Uint8Array(sha(json))],
        [
          2,
          map([
            ['id', 'webauthn.io'],
            ['name', 'webauthn.io']
          ])
        ],
        [
          3,
          map([
            ['id', userId],
            ['name', 'alice'],
            ['displayName', 'Alice']
          ])
        ],
        [
          4,
          [
            map([
              ['alg', -7],
              ['type', 'public-key']
            ]),
            map([
              ['alg', -257],
              ['type', 'public-key']
            ])
          ]
        ],
        [
          7,
          map([
            ['rk', true],
            ['uv', true]
          ])
        ]
      ])
    )
    expect(plan.residentKey).toBe(true)
    expect(plan.credProps).toBe(true)
    expect(plan.attestation).toBe('none')
  })

  it('keeps an empty displayName (phones refuse a request without one)', () => {
    const plan = planCtap(createRequest({ user: { id: b64(userId), name: 'a', displayName: '' } }), phone)
    expect((decoded(plan).get(3) as Map<CborValue, CborValue>).get('displayName')).toBe('')
    const noName = planCtap(createRequest({ user: { id: b64(userId), name: 'a' } }), phone)
    expect((decoded(noName).get(3) as Map<CborValue, CborValue>).get('displayName')).toBe('')
  })

  it('the rp id is the effective one, not the page string', () => {
    const request = createRequest({ rp: { name: 'Example' } })
    expect((decoded(planCtap(request, phone)).get(2) as Map<CborValue, CborValue>).get('id')).toBe('webauthn.io')
  })

  it('resident key: required, preferred (only if the phone can), discouraged, legacy flag', () => {
    const rk = (selection: Record<string, unknown>, info: DeviceInfo = phone): boolean =>
      planCtap(createRequest({ authenticatorSelection: selection }), info).residentKey
    expect(rk({ residentKey: 'required' })).toBe(true)
    expect(rk({ residentKey: 'preferred' })).toBe(true)
    expect(rk({ residentKey: 'preferred' }, { ...phone, rk: false })).toBe(false)
    expect(rk({ residentKey: 'preferred' }, { ...phone, uv: false })).toBe(false)
    expect(rk({ residentKey: 'discouraged' })).toBe(false)
    expect(rk({ requireResidentKey: true })).toBe(true)
    expect(rk({})).toBe(false)
    expect(() => rk({ residentKey: 'required' }, { ...phone, rk: false })).toThrow(CtapFailure)
  })

  it('options: no rk when discouraged, uv when configured, nothing for a phone without uv', () => {
    const options = (selection: Record<string, unknown>, info: DeviceInfo = phone) =>
      decoded(planCtap(createRequest({ authenticatorSelection: selection }), info)).get(7)
    expect(options({ residentKey: 'discouraged' })).toEqual(map([['uv', true]]))
    expect(options({ residentKey: 'discouraged' }, { ...phone, makeCredUvNotRqd: true })).toBeUndefined()
    expect(options({ residentKey: 'discouraged' }, { ...phone, uv: false })).toBeUndefined()
  })

  it('fails when user verification is required but unavailable', () => {
    expect(() => planCtap(createRequest({ authenticatorSelection: { userVerification: 'required' } }), { ...phone, uv: false })).toThrow(CtapFailure)
  })

  it('sends the exclude list, skips unknown parameter types, defaults an empty list', () => {
    const plan = planCtap(
      createRequest({
        excludeCredentials: [{ type: 'public-key', id: 'AQID', transports: ['usb'] }],
        pubKeyCredParams: [
          { type: 'other', alg: 1 },
          { type: 'public-key', alg: -8 }
        ]
      }),
      phone
    )
    expect(decoded(plan).get(5)).toEqual([
      map([
        ['id', Uint8Array.of(1, 2, 3)],
        ['type', 'public-key']
      ])
    ])
    expect(decoded(plan).get(4)).toEqual([
      map([
        ['alg', -8],
        ['type', 'public-key']
      ])
    ])
    expect(decoded(planCtap(createRequest({ pubKeyCredParams: [] }), phone)).get(4)).toEqual([
      map([
        ['alg', -7],
        ['type', 'public-key']
      ]),
      map([
        ['alg', -257],
        ['type', 'public-key']
      ])
    ])
    expect(() => planCtap(createRequest({ pubKeyCredParams: [{ type: 'other', alg: 1 }] }), phone)).toThrow(CtapFailure)
    expect(decoded(planCtap(createRequest(), phone)).has(5)).toBe(false)
  })
})

/** Authenticator data, optionally with attested credential data for an ES256 key. */
function authData(opts: {
  rpId: string
  flags?: number
  id?: Buffer
  aaguid?: Buffer
  extensions?: Uint8Array
  key?: Map<CborValue, CborValue>
  trailing?: Buffer
}): {
  bytes: Buffer
  key: Map<CborValue, CborValue>
  spki: Buffer
} {
  const { publicKey, privateKey } = generateKeyPairSync('ec', { namedCurve: 'P-256' })
  void privateKey
  const jwk = publicKey.export({ format: 'jwk' })
  const key =
    opts.key ??
    map([
      [1, 2],
      [3, -7],
      [-1, 1],
      [-2, new Uint8Array(Buffer.from(jwk.x!, 'base64url'))],
      [-3, new Uint8Array(Buffer.from(jwk.y!, 'base64url'))]
    ])
  const attested = opts.id !== undefined
  const flags = opts.flags ?? 0x01 | 0x04 | (attested ? 0x40 : 0) | (opts.extensions ? 0x80 : 0)
  const parts: Buffer[] = [sha(opts.rpId), Buffer.of(flags), Buffer.from([0, 0, 0, 5])]
  if (attested) {
    const len = Buffer.alloc(2)
    len.writeUInt16BE(opts.id!.length)
    parts.push(opts.aaguid ?? Buffer.alloc(16, 0xaa), len, opts.id!, Buffer.from(encodeCbor(key)))
  }
  if (opts.extensions) parts.push(Buffer.from(opts.extensions))
  if (opts.trailing) parts.push(opts.trailing)
  return { bytes: Buffer.concat(parts), key, spki: publicKey.export({ type: 'spki', format: 'der' }) as Buffer }
}

const ok = (m: Map<CborValue, CborValue>): Buffer => Buffer.concat([Buffer.of(0), Buffer.from(encodeCbor(m))])
const credentialId = Buffer.alloc(32, 3)

describe('parseCtapResponse: getAssertion', () => {
  const plan = (publicKey: Record<string, unknown> = {}): CtapPlan => planCtap(getRequest(publicKey), phone)
  const response = (p: CtapPlan, over: { auth?: Buffer; rpId?: string; map?: [CborValue, CborValue][] } = {}): Buffer => {
    const auth = over.auth ?? authData({ rpId: over.rpId ?? 'login.microsoftonline.com' }).bytes
    return ok(
      map([
        [
          1,
          map([
            ['id', new Uint8Array(credentialId)],
            ['type', 'public-key']
          ])
        ],
        [2, new Uint8Array(auth)],
        [3, Uint8Array.of(0x30, 1, 2)],
        [4, map([['id', userId]])],
        ...(over.map ?? [])
      ])
    )
  }

  it('builds the credential (toJSON form)', () => {
    const p = plan()
    const auth = authData({ rpId: 'login.microsoftonline.com' }).bytes
    const credential = parseCtapResponse(p, response(p, { auth }))
    expect(credential).toEqual({
      id: b64(credentialId),
      rawId: b64(credentialId),
      type: 'public-key',
      authenticatorAttachment: 'cross-platform',
      response: {
        clientDataJSON: b64(Buffer.from(clientDataJson('get', challenge, 'https://login.microsoftonline.com'))),
        authenticatorData: b64(auth),
        signature: b64(Uint8Array.of(0x30, 1, 2)),
        userHandle: b64(userId)
      },
      clientExtensionResults: {}
    })
  })

  it('takes the credential from the allow list when the phone leaves it out (single entry)', () => {
    const p = plan({ allowCredentials: [{ type: 'public-key', id: b64(credentialId) }] })
    const m = ok(
      map([
        [2, new Uint8Array(authData({ rpId: 'login.microsoftonline.com' }).bytes)],
        [3, Uint8Array.of(1)]
      ])
    )
    expect(parseCtapResponse(p, m).id).toBe(b64(credentialId))
    expect('userHandle' in parseCtapResponse(p, m).response).toBe(false)
  })

  it.each([
    ['rpIdHash of another site', (p: CtapPlan) => response(p, { rpId: 'evil.example' })],
    ['user presence flag missing', (p: CtapPlan) => response(p, { auth: authData({ rpId: 'login.microsoftonline.com', flags: 0x04 }).bytes })],
    [
      'attested credential data in an assertion',
      (p: CtapPlan) => response(p, { auth: authData({ rpId: 'login.microsoftonline.com', id: credentialId }).bytes })
    ],
    ['authenticator data too short', (p: CtapPlan) => response(p, { auth: Buffer.alloc(36, 1) })],
    ['trailing authenticator data', (p: CtapPlan) => response(p, { auth: authData({ rpId: 'login.microsoftonline.com', trailing: Buffer.of(1) }).bytes })],
    ['empty response', () => Buffer.alloc(0)],
    ['status only', () => Buffer.of(0)],
    ['CBOR that is not a map', () => Buffer.from([0, 0x01])],
    ['truncated CBOR', () => Buffer.from([0, 0xa1, 0x02])],
    ['trailing bytes after the map', (p: CtapPlan) => Buffer.concat([response(p), Buffer.of(0)])]
  ])('rejects %s with NotAllowedError', (_name, make) => {
    const p = plan()
    try {
      parseCtapResponse(p, make(p))
      throw new Error('accepted')
    } catch (e) {
      expect(e).toBeInstanceOf(CtapFailure)
      expect((e as CtapFailure).error.name).toBe('NotAllowedError')
    }
  })

  it('rejects a credential that is not in the allow list, and a missing user without allow list', () => {
    const other = planCtap(getRequest({ allowCredentials: [{ type: 'public-key', id: 'AQID' }] }), phone)
    expect(() => parseCtapResponse(other, response(other))).toThrow(CtapFailure)
    const p = plan()
    const noUser = ok(
      map([
        [1, map([['id', new Uint8Array(credentialId)]])],
        [2, new Uint8Array(authData({ rpId: 'login.microsoftonline.com' }).bytes)],
        [3, Uint8Array.of(1)]
      ])
    )
    expect(() => parseCtapResponse(p, noUser)).toThrow(CtapFailure)
  })

  it('rejects a missing credential when the allow list has several', () => {
    const p = planCtap(
      getRequest({
        allowCredentials: [
          { type: 'public-key', id: 'AQID' },
          { type: 'public-key', id: 'BAUG' }
        ]
      }),
      phone
    )
    const m = ok(
      map([
        [2, new Uint8Array(authData({ rpId: 'login.microsoftonline.com' }).bytes)],
        [3, Uint8Array.of(1)]
      ])
    )
    expect(() => parseCtapResponse(p, m)).toThrow(CtapFailure)
  })
})

describe('parseCtapResponse: makeCredential', () => {
  const attestationResponse = (
    a: ReturnType<typeof authData>,
    fmt = 'packed',
    stmt: Map<CborValue, CborValue> = map([
      ['alg', -7],
      ['sig', Uint8Array.of(1, 2)]
    ])
  ): Buffer =>
    ok(
      map([
        [1, fmt],
        [2, new Uint8Array(a.bytes)],
        [3, stmt]
      ])
    )

  it('builds the credential with a "none" attestation, the AAGUID kept', () => {
    const a = authData({ rpId: 'webauthn.io', id: credentialId, aaguid: Buffer.alloc(16, 0x42) })
    const credential = parseCtapResponse(planCtap(createRequest(), phone), attestationResponse(a))
    expect(credential.id).toBe(b64(credentialId))
    expect(credential.rawId).toBe(b64(credentialId))
    expect(credential.authenticatorAttachment).toBe('cross-platform')
    const r = credential.response as {
      clientDataJSON: string
      attestationObject: string
      authenticatorData: string
      transports: string[]
      publicKey: string
      publicKeyAlgorithm: number
    }
    expect(r.clientDataJSON).toBe(b64(Buffer.from(clientDataJson('create', challenge, 'https://webauthn.io'))))
    expect(r.authenticatorData).toBe(b64(a.bytes))
    expect(r.transports).toEqual(['hybrid', 'internal'])
    expect(r.publicKeyAlgorithm).toBe(-7)
    expect(r.publicKey).toBe(b64(a.spki))
    const object = Buffer.from(r.attestationObject, 'base64url')
    // a3 63 fmt 64 none 67 attStmt a0 68 authData 58 <len> ...
    expect(object.toString('hex').startsWith('a3' + '63666d74' + '646e6f6e65' + '6761747453746d74' + 'a0' + '686175746844617461')).toBe(true)
    const decodedObject = decodeCbor(object) as Map<CborValue, CborValue>
    expect(decodedObject.get('fmt')).toBe('none')
    expect(Buffer.from(decodedObject.get('authData') as Uint8Array).equals(a.bytes)).toBe(true)
    expect(Buffer.from(a.bytes.subarray(37, 53)).equals(Buffer.alloc(16, 0x42))).toBe(true)
  })

  it('keeps the phone attestation when it was asked for (direct)', () => {
    const a = authData({ rpId: 'webauthn.io', id: credentialId })
    const credential = parseCtapResponse(planCtap(createRequest({ attestation: 'direct' }), phone), attestationResponse(a))
    const object = decodeCbor(Buffer.from((credential.response as { attestationObject: string }).attestationObject, 'base64url')) as Map<CborValue, CborValue>
    expect(object.get('fmt')).toBe('packed')
    expect(object.get('attStmt')).toEqual(
      map([
        ['alg', -7],
        ['sig', Uint8Array.of(1, 2)]
      ])
    )
  })

  it('transports: the phone list plus hybrid, sorted; unknown ones dropped', () => {
    const a = authData({ rpId: 'webauthn.io', id: credentialId })
    const t = (transports: string[] | undefined) =>
      (parseCtapResponse(planCtap(createRequest(), { ...phone, transports }), attestationResponse(a)).response as { transports: string[] }).transports
    expect(t(['usb', 'internal', 'x'])).toEqual(['hybrid', 'internal', 'usb'])
    expect(t(['hybrid'])).toEqual(['hybrid'])
    expect(t(undefined)).toEqual(['hybrid', 'internal'])
  })

  it('public key: SPKI for ES256, EdDSA and RS256, none for other algorithms', () => {
    const rsa = generateKeyPairSync('rsa', { modulusLength: 2048 })
    const rsaJwk = rsa.publicKey.export({ format: 'jwk' })
    const rsaKey = map([
      [1, 3],
      [3, -257],
      [-1, new Uint8Array(Buffer.from(rsaJwk.n!, 'base64url'))],
      [-2, new Uint8Array(Buffer.from(rsaJwk.e!, 'base64url'))]
    ])
    const ed = generateKeyPairSync('ed25519')
    const edKey = map([
      [1, 1],
      [3, -8],
      [-1, 6],
      [-2, new Uint8Array(Buffer.from(ed.publicKey.export({ format: 'jwk' }).x!, 'base64url'))]
    ])
    const unknown = map([
      [1, 2],
      [3, -35],
      [-1, 2],
      [-2, Uint8Array.of(1)],
      [-3, Uint8Array.of(2)]
    ])
    const run = (key: Map<CborValue, CborValue>) =>
      parseCtapResponse(planCtap(createRequest(), phone), attestationResponse(authData({ rpId: 'webauthn.io', id: credentialId, key }))).response as {
        publicKey?: string
        publicKeyAlgorithm: number
      }
    expect(run(rsaKey)).toMatchObject({ publicKeyAlgorithm: -257, publicKey: b64(rsa.publicKey.export({ type: 'spki', format: 'der' }) as Buffer) })
    expect(run(edKey)).toMatchObject({ publicKeyAlgorithm: -8, publicKey: b64(ed.publicKey.export({ type: 'spki', format: 'der' }) as Buffer) })
    const other = run(unknown)
    expect(other.publicKeyAlgorithm).toBe(-35)
    expect(other.publicKey).toBeUndefined()
  })

  it('credProps: rk true when a resident key was asked for, false when the phone cannot, nothing otherwise', () => {
    const a = authData({ rpId: 'webauthn.io', id: credentialId })
    const results = (request: WebAuthnRequest, info: DeviceInfo) => parseCtapResponse(planCtap(request, info), attestationResponse(a)).clientExtensionResults
    expect(results(createRequest(), phone)).toEqual({ credProps: { rk: true } })
    const discouraged = createRequest({ authenticatorSelection: { residentKey: 'discouraged' } })
    expect(results(discouraged, phone)).toEqual({ credProps: {} })
    expect(results(discouraged, { ...phone, versions: ['FIDO_2_1'] })).toEqual({ credProps: { rk: false } })
    expect(results(discouraged, { ...phone, rk: false })).toEqual({ credProps: { rk: false } })
    expect(results(createRequest({ extensions: {} }), phone)).toEqual({})
  })

  it.each([
    ['rpIdHash of another site', () => attestationResponse(authData({ rpId: 'evil.example', id: credentialId }))],
    ['no attested credential data', () => attestationResponse(authData({ rpId: 'webauthn.io' }))],
    ['user presence flag missing', () => attestationResponse(authData({ rpId: 'webauthn.io', id: credentialId, flags: 0x44 }))],
    ['a credential id of 0 bytes', () => attestationResponse(authData({ rpId: 'webauthn.io', id: Buffer.alloc(0) }))],
    ['a credential id over 1023 bytes', () => attestationResponse(authData({ rpId: 'webauthn.io', id: Buffer.alloc(1024, 1) }))],
    [
      'a public key that is not a map',
      () => attestationResponse(authData({ rpId: 'webauthn.io', id: credentialId, key: 5 as unknown as Map<CborValue, CborValue> }))
    ],
    [
      'a missing statement',
      () =>
        ok(
          map([
            [1, 'none'],
            [2, new Uint8Array(authData({ rpId: 'webauthn.io', id: credentialId }).bytes)]
          ])
        )
    ],
    [
      'a format that is not text',
      () =>
        ok(
          map([
            [1, 5],
            [2, new Uint8Array(authData({ rpId: 'webauthn.io', id: credentialId }).bytes)],
            [3, map([])]
          ])
        )
    ]
  ])('rejects %s', (_name, make) => {
    try {
      parseCtapResponse(planCtap(createRequest(), phone), make())
      throw new Error('accepted')
    } catch (e) {
      expect(e).toBeInstanceOf(CtapFailure)
      expect((e as CtapFailure).error.name).toBe('NotAllowedError')
    }
  })
})

describe('CTAP errors', () => {
  const plan = planCtap(createRequest(), phone)

  it('credential excluded -> InvalidStateError', () => {
    const e = mapCtapStatus(0x19)
    expect(e.name).toBe('InvalidStateError')
    expect(() => parseCtapResponse(plan, Buffer.of(0x19))).toThrow(CtapFailure)
    try {
      parseCtapResponse(plan, Buffer.of(0x19))
    } catch (err) {
      expect((err as CtapFailure).error.name).toBe('InvalidStateError')
      expect((err as CtapFailure).reason).toBe('CTAP status 0x19')
    }
  })

  it.each([0x01, 0x12, 0x2e, 0x27, 0x2f, 0x30, 0x31, 0x32, 0x36, 0x3b, 0x3c, 0x7f, 0xf0, 0xff])('status 0x%s -> NotAllowedError', (status) => {
    expect(mapCtapStatus(status)).toMatchObject({ name: 'NotAllowedError', message: expect.stringContaining('timed out or was not allowed') })
    try {
      parseCtapResponse(plan, Buffer.of(status, 0xa0))
    } catch (err) {
      expect((err as CtapFailure).error.name).toBe('NotAllowedError')
    }
  })
})

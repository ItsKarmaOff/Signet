'use strict'
const test = require('node:test')
const assert = require('node:assert/strict')
const { load, makeBridge } = require('./harness')

function setup() {
  const originalGet = async function get() {
    return 'original-get'
  }
  const originalCreate = async function create() {
    return 'original-create'
  }
  class CredentialsContainer {}
  Object.defineProperty(CredentialsContainer.prototype, 'get', { value: originalGet, writable: true, enumerable: true, configurable: true })
  Object.defineProperty(CredentialsContainer.prototype, 'create', { value: originalCreate, writable: true, enumerable: true, configurable: true })
  class PublicKeyCredential {}
  PublicKeyCredential.isUserVerifyingPlatformAuthenticatorAvailable = async () => true
  PublicKeyCredential.isConditionalMediationAvailable = async () => true
  PublicKeyCredential.getClientCapabilities = async () => ({ conditionalGet: true, hybridTransport: true })
  class AuthenticatorAttestationResponse {}
  class AuthenticatorAssertionResponse {}
  class DOMException extends Error {
    constructor(message, name) {
      super(message)
      this.name = name
    }
  }
  const credentials = new CredentialsContainer()
  const b = makeBridge()
  const g = {
    top: null,
    JSON,
    Promise,
    navigator: { credentials },
    PublicKeyCredential,
    AuthenticatorAttestationResponse,
    AuthenticatorAssertionResponse,
    DOMException,
    ArrayBuffer,
    btoa: (s) => Buffer.from(s, 'binary').toString('base64'),
    atob: (s) => Buffer.from(s, 'base64').toString('binary')
  }
  g.top = g
  load('webauthn.js')(b.bridge, {}, g)
  return { b, g, credentials, PublicKeyCredential, AuthenticatorAssertionResponse, AuthenticatorAttestationResponse, originalGet, originalCreate }
}

const bytes = (...n) => new Uint8Array(n)
const b64 = (u8) => Buffer.from(u8).toString('base64url')
const settled = () => new Promise((r) => setImmediate(r))

test('other credential types go to the original function', async () => {
  const t = setup()
  assert.equal(await t.credentials.get({ password: true }), 'original-get')
  assert.equal(await t.credentials.create(undefined), 'original-create')
  assert.equal(await t.credentials.get(null), 'original-get')
  assert.equal(t.b.posted.length, 0)
})

test('replaced methods look native and keep their descriptors', () => {
  const t = setup()
  const proto = Object.getPrototypeOf(t.credentials)
  const d = Object.getOwnPropertyDescriptor(proto, 'get')
  assert.equal(d.enumerable, true)
  assert.equal(d.writable, true)
  assert.equal(d.value.name, 'get')
  assert.notEqual(d.value, t.originalGet)
  assert.match(Function.prototype.toString.call(d.value), /\[native code\]/)
})

test('a get request is sent as plain data and answered with a credential', async () => {
  const t = setup()
  const challenge = bytes(1, 2, 3, 250)
  const p = t.credentials.get({
    publicKey: { challenge: challenge.buffer, allowCredentials: [{ type: 'public-key', id: bytes(9, 9), transports: ['hybrid'] }], timeout: 1, onbad() {} },
    mediation: 'optional'
  })
  await settled()
  const [channel, env] = t.b.posted[0]
  assert.equal(channel, 'webauthn:request')
  assert.deepEqual(env.data, {
    type: 'get',
    mediation: 'optional',
    publicKey: { challenge: b64(challenge), allowCredentials: [{ type: 'public-key', id: b64(bytes(9, 9)), transports: ['hybrid'] }], timeout: 1 }
  })
  t.b.reply(env.rid, {
    credential: {
      id: b64(bytes(7)),
      rawId: b64(bytes(7)),
      type: 'public-key',
      authenticatorAttachment: 'cross-platform',
      clientExtensionResults: {},
      response: { clientDataJSON: b64(bytes(1)), authenticatorData: b64(bytes(2)), signature: b64(bytes(3)), userHandle: b64(bytes(4)) }
    }
  })
  const cred = await p
  assert.ok(cred instanceof t.PublicKeyCredential)
  assert.ok(cred.response instanceof t.AuthenticatorAssertionResponse)
  assert.deepEqual(new Uint8Array(cred.rawId), bytes(7))
  assert.deepEqual(new Uint8Array(cred.response.signature), bytes(3))
  assert.deepEqual(new Uint8Array(cred.response.userHandle), bytes(4))
  assert.equal(cred.authenticatorAttachment, 'cross-platform')
  assert.deepEqual(cred.getClientExtensionResults(), {})
  assert.equal(JSON.stringify(cred), '{"id":"' + b64(bytes(7)) + '","rawId":"' + b64(bytes(7)) + '","type":"public-key","authenticatorAttachment":"cross-platform","clientExtensionResults":{},"response":{"clientDataJSON":"' + b64(bytes(1)) + '","authenticatorData":"' + b64(bytes(2)) + '","signature":"' + b64(bytes(3)) + '","userHandle":"' + b64(bytes(4)) + '"}}')
  assert.deepEqual(Object.keys(cred), [])
})

test('a create request gives an attestation response', async () => {
  const t = setup()
  const p = t.credentials.create({ publicKey: { challenge: bytes(1), rp: { name: 'x' }, user: { id: bytes(5), name: 'n', displayName: 'n' }, pubKeyCredParams: [] } })
  await settled()
  const env = t.b.posted[0][1]
  assert.equal(env.data.type, 'create')
  assert.equal(env.data.publicKey.user.id, b64(bytes(5)))
  t.b.reply(env.rid, {
    credential: {
      id: 'x', rawId: b64(bytes(1)), type: 'public-key', clientExtensionResults: {},
      response: { clientDataJSON: b64(bytes(1)), authenticatorData: b64(bytes(2)), attestationObject: b64(bytes(3)), transports: ['hybrid'], publicKeyAlgorithm: -7, publicKey: b64(bytes(4)) }
    }
  })
  const cred = await p
  assert.ok(cred.response instanceof t.AuthenticatorAttestationResponse)
  assert.deepEqual(cred.response.getTransports(), ['hybrid'])
  assert.equal(cred.response.getPublicKeyAlgorithm(), -7)
  assert.deepEqual(new Uint8Array(cred.response.getPublicKey()), bytes(4))
})

test('an error answer rejects with a DOMException of that name', async () => {
  const t = setup()
  const p = t.credentials.get({ publicKey: { challenge: bytes(1) } })
  await settled()
  t.b.reply(t.b.posted[0][1].rid, { error: { name: 'SecurityError', message: 'The operation is insecure.' } })
  await assert.rejects(p, (e) => e.name === 'SecurityError' && e.message === 'The operation is insecure.')
})

test('a type error answer rejects with a TypeError', async () => {
  const t = setup()
  const p = t.credentials.get({ publicKey: { challenge: bytes(1) } })
  await settled()
  t.b.reply(t.b.posted[0][1].rid, { error: { name: 'TypeError', message: 'bad' } })
  await assert.rejects(p, TypeError)
})

test('an empty, broken or failed answer is NotAllowedError', async () => {
  for (const answer of [null, {}, { credential: { id: 'x', rawId: 5, response: {} } }]) {
    const t = setup()
    const p = t.credentials.get({ publicKey: { challenge: bytes(1) } })
    await settled()
    t.b.reply(t.b.posted[0][1].rid, answer)
    await assert.rejects(p, (e) => e.name === 'NotAllowedError')
  }
  const t = setup()
  const p = t.credentials.get({ publicKey: { challenge: bytes(1) } })
  await settled()
  t.b.fail(t.b.posted[0][1].rid, 'timeout')
  await assert.rejects(p, (e) => e.name === 'NotAllowedError')
})

test('aborting a pending request rejects with the reason and tells the browser', async () => {
  const t = setup()
  const listeners = []
  const signal = { aborted: false, reason: new Error('stop'), addEventListener: (_t, fn) => listeners.push(fn), removeEventListener: () => {} }
  const p = t.credentials.get({ publicKey: { challenge: bytes(1) }, signal })
  await settled()
  assert.equal(t.b.posted.length, 1)
  listeners[0]()
  await assert.rejects(p, /stop/)
  assert.deepEqual(t.b.posted.at(-1), ['webauthn:abort', { data: null }])
})

test('an already aborted signal rejects at once without asking', async () => {
  const t = setup()
  const signal = { aborted: true, reason: new Error('gone'), addEventListener() {}, removeEventListener() {} }
  await assert.rejects(t.credentials.get({ publicKey: { challenge: bytes(1) }, signal }), /gone/)
  assert.equal(t.b.posted.length, 0)
})

test('conditional mediation stays pending without asking anyone', async () => {
  const t = setup()
  const listeners = []
  const signal = { aborted: false, reason: new Error('nav'), addEventListener: (_t, fn) => listeners.push(fn), removeEventListener() {} }
  const p = t.credentials.get({ publicKey: { challenge: bytes(1) }, mediation: 'conditional', signal })
  await settled()
  assert.equal(t.b.posted.length, 0)
  listeners[0]()
  await assert.rejects(p, /nav/)
  // Nothing was asked, so there is nothing to cancel.
  assert.equal(t.b.posted.length, 0)
})

test('options that cannot be turned into plain data reject', async () => {
  const t = setup()
  const deep = {}
  let cur = deep
  for (let i = 0; i < 30; i++) cur = cur.x = {}
  await assert.rejects(t.credentials.get({ publicKey: { challenge: bytes(1), extensions: deep } }), TypeError)
  assert.equal(t.b.posted.length, 0)
})

test('this browser has no platform authenticator and says so', async () => {
  const t = setup()
  assert.equal(await t.PublicKeyCredential.isUserVerifyingPlatformAuthenticatorAvailable(), false)
  assert.equal(await t.PublicKeyCredential.isConditionalMediationAvailable(), false)
  assert.deepEqual(await t.PublicKeyCredential.getClientCapabilities(), { conditionalGet: false, hybridTransport: false })
})

test('nothing happens without navigator.credentials or in a subframe', () => {
  const b = makeBridge()
  const g = { navigator: {}, PublicKeyCredential: class {} }
  g.top = g
  load('webauthn.js')(b.bridge, {}, g)
  const sub = { top: {}, navigator: { credentials: { get() {} } }, PublicKeyCredential: class {} }
  const before = sub.navigator.credentials.get
  load('webauthn.js')(b.bridge, {}, sub)
  assert.equal(sub.navigator.credentials.get, before)
})

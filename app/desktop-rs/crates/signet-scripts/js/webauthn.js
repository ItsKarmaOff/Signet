// Port of preload/webauthn.ts. Intercepts passkey requests. Main frame only, document start.
// `navigator.credentials.get/create` with `publicKey` are sent to the browser side (`webauthn:request`), which opens a
// session with a phone (QR code, hybrid transport); anything else goes to the original function. Only injected where
// the platform has no native passkey support to lose (Linux, or on demand in development), see `webauthn_intercept_enabled`.
// Channels: webauthn:request (request, long timeout), webauthn:abort (send).
(function webauthn(bridge, config, g) {
  'use strict'
  g = g || globalThis
  if (g.top !== g) return

  // Same timeout as the hybrid session of the browser side, plus a margin.
  const REQUEST_TIMEOUT = 10 * 60 * 1000
  const ask = (payload) => bridge.invoke('webauthn:request', payload, REQUEST_TIMEOUT)
  // The page gave up on a pending request (AbortSignal): the browser side ends its session (and closes the QR code window).
  const cancel = () => bridge.send('webauthn:abort', null)

  const credentials = g.navigator.credentials
  const PKC = g.PublicKeyCredential
  if (!credentials || !PKC) return

  const PromiseCtor = g.Promise
  const stringify = g.JSON.stringify.bind(g.JSON)
  const parse = g.JSON.parse.bind(g.JSON)

  // A bound function displays as `function () { [native code] }`, like Chrome's native functions.
  const native = (name, fn) => {
    const bound = fn.bind(null)
    Object.defineProperty(bound, 'name', { value: name })
    return bound
  }
  // Replaces a method where it lives, keeping writable / enumerable / configurable.
  const replace = (target, key, make) => {
    const d = Object.getOwnPropertyDescriptor(target, key)
    if (!d || typeof d.value !== 'function') return
    Object.defineProperty(target, key, { ...d, value: native(key, make(d.value)) })
  }

  const toBase64Url = (bytes) => {
    let bin = ''
    for (let i = 0; i < bytes.length; i += 0x8000) bin += String.fromCharCode(...bytes.subarray(i, i + 0x8000))
    return g.btoa(bin).replace(/\+/g, '-').replace(/\//g, '_').replace(/=+$/, '')
  }
  // Plain data: BufferSource -> base64url string, functions and undefined dropped.
  const toPlain = (v, depth = 0) => {
    if (depth > 20) throw new TypeError('The options are too deeply nested.')
    if (v === null || typeof v === 'string' || typeof v === 'number' || typeof v === 'boolean') return v
    if (typeof v !== 'object') return undefined
    const tag = Object.prototype.toString.call(v)
    if (tag === '[object ArrayBuffer]') return toBase64Url(new Uint8Array(v))
    if (g.ArrayBuffer.isView(v)) return toBase64Url(new Uint8Array(v.buffer, v.byteOffset, v.byteLength))
    if (Array.isArray(v)) return v.map((x) => toPlain(x, depth + 1)).filter((x) => x !== undefined)
    const out = {}
    for (const key of Object.keys(v)) {
      const x = toPlain(v[key], depth + 1)
      if (x !== undefined) out[key] = x
    }
    return out
  }

  const fromBase64Url = (s) => {
    const bin = g.atob(s.replace(/-/g, '+').replace(/_/g, '/'))
    const out = new Uint8Array(bin.length)
    for (let i = 0; i < bin.length; i++) out[i] = bin.charCodeAt(i)
    return out.buffer
  }
  // Own, non-enumerable properties on an object whose prototype is the native one: instanceof works, the native getters
  // (which need internal slots) are shadowed, and Object.keys / JSON.stringify show nothing, like the real ones.
  const define = (target, props) => {
    for (const key of Object.keys(props)) {
      Object.defineProperty(target, key, { value: props[key], enumerable: false, configurable: true, writable: false })
    }
  }
  const protoOf = (name, fallback) => {
    const ctor = g[name]
    return (ctor && ctor.prototype) || fallback
  }
  // The credential from the browser side (plain base64url data) as an object that behaves like a PublicKeyCredential.
  const buildCredential = (json) => {
    const copy = () => parse(stringify(json))
    const r = json.response
    const isCreate = 'attestationObject' in r
    const response = Object.create(protoOf(isCreate ? 'AuthenticatorAttestationResponse' : 'AuthenticatorAssertionResponse', Object.prototype))
    const common = { clientDataJSON: fromBase64Url(r.clientDataJSON), authenticatorData: fromBase64Url(r.authenticatorData) }
    if (isCreate) {
      const publicKey = r.publicKey === undefined ? null : fromBase64Url(r.publicKey)
      const transports = Array.isArray(r.transports) ? [...r.transports] : []
      define(response, {
        clientDataJSON: common.clientDataJSON,
        attestationObject: fromBase64Url(r.attestationObject),
        getTransports: native('getTransports', () => [...transports]),
        getAuthenticatorData: native('getAuthenticatorData', () => common.authenticatorData),
        getPublicKey: native('getPublicKey', () => publicKey),
        getPublicKeyAlgorithm: native('getPublicKeyAlgorithm', () => (r.publicKeyAlgorithm === undefined ? -1 : r.publicKeyAlgorithm))
      })
    } else {
      define(response, {
        clientDataJSON: common.clientDataJSON,
        authenticatorData: common.authenticatorData,
        signature: fromBase64Url(r.signature),
        userHandle: r.userHandle === undefined ? null : fromBase64Url(r.userHandle)
      })
    }
    const credential = Object.create(PKC.prototype)
    define(credential, {
      id: json.id,
      rawId: fromBase64Url(json.rawId),
      type: json.type,
      authenticatorAttachment: json.authenticatorAttachment,
      response,
      getClientExtensionResults: native('getClientExtensionResults', () => ({ ...json.clientExtensionResults })),
      toJSON: native('toJSON', copy)
    })
    return credential
  }

  const NOT_ALLOWED =
    'The operation either timed out or was not allowed. See: https://www.w3.org/TR/webauthn-2/#sctn-privacy-considerations-client.'
  const failure = (name, message) => (name === 'TypeError' ? new TypeError(message) : new g.DOMException(message, name))

  const intercept = (type) => (original) => (...args) => {
    const options = args[0]
    if (options === null || typeof options !== 'object' || options.publicKey === undefined) {
      return original.apply(credentials, args)
    }
    return new PromiseCtor((resolve, reject) => {
      const signal = options.signal
      if (signal && signal.aborted) return reject(signal.reason)
      let done = false
      let asked = false
      const settle = (fn, v) => {
        if (done) return
        done = true
        if (signal) signal.removeEventListener('abort', onAbort)
        fn(v)
      }
      const onAbort = () => {
        const pending = asked && !done
        settle(reject, signal && signal.reason)
        if (pending) cancel()
      }
      if (signal) signal.addEventListener('abort', onAbort)
      // Conditional UI: pending until aborted, nothing to ask.
      if (type === 'get' && options.mediation === 'conditional') return
      let payload
      try {
        payload = { type, mediation: options.mediation, publicKey: toPlain(options.publicKey) }
      } catch (e) {
        return settle(reject, e)
      }
      asked = true
      ask(payload).then(
        (answer) => {
          if (answer && answer.error) settle(reject, failure(String(answer.error.name), String(answer.error.message)))
          else if (answer && answer.credential) {
            try {
              settle(resolve, buildCredential(answer.credential))
            } catch {
              settle(reject, failure('NotAllowedError', NOT_ALLOWED))
            }
          } else settle(reject, failure('NotAllowedError', NOT_ALLOWED))
        },
        () => settle(reject, failure('NotAllowedError', NOT_ALLOWED))
      )
    })
  }

  const proto = Object.getPrototypeOf(credentials)
  replace(proto, 'get', intercept('get'))
  replace(proto, 'create', intercept('create'))

  // No authenticator here: sites must not offer passkeys as the first choice.
  replace(PKC, 'isUserVerifyingPlatformAuthenticatorAvailable', () => () => PromiseCtor.resolve(false))
  replace(PKC, 'isConditionalMediationAvailable', () => () => PromiseCtor.resolve(false))
  const KNOWN = [
    'conditionalCreate', 'conditionalGet', 'hybridTransport', 'passkeyPlatformAuthenticator', 'relatedOrigins',
    'userVerifyingPlatformAuthenticator', 'extension:appid', 'extension:credBlob', 'extension:credProps',
    'extension:hmacCreateSecret', 'extension:largeBlob', 'extension:minPinLength', 'extension:prf',
    'signalAllAcceptedCredentials', 'signalCurrentUserDetails', 'signalUnknownCredential'
  ]
  replace(PKC, 'getClientCapabilities', (original) => () => {
    const allFalse = (keys) => Object.fromEntries(keys.map((k) => [k, false]))
    try {
      return PromiseCtor.resolve(original.call(PKC)).then(
        (caps) => allFalse(Object.keys(caps || {})),
        () => allFalse(KNOWN)
      )
    } catch {
      return PromiseCtor.resolve(allFalse(KNOWN))
    }
  })
})

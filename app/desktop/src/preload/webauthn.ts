/// <reference lib="dom" />
// Page script (registered on the tabs' sessions, all frames, where passkeys are intercepted): Electron never answers
// WebAuthn requests there and the page waits forever. `navigator.credentials.get/create` with `publicKey` are sent to the
// main process (`webauthn:request`), the rest goes to the original. Runs in the page's world, before its scripts; the
// isolated world only hands over a function (contextBridge proxies it and its promise), nothing is exposed on `window`.
import { contextBridge, ipcRenderer } from 'electron'
import type { AssertionResponseJson, AttestationResponseJson, CredentialJson } from '../main/web/hybrid/ctap'

const request = (payload: unknown): Promise<unknown> => ipcRenderer.invoke('webauthn:request', payload)
// The page gave up on a pending request (AbortSignal): main ends its session (and closes the QR code window).
const abort = (): void => ipcRenderer.send('webauthn:abort')

try {
  contextBridge.executeInMainWorld({
    func: (ask: (payload: unknown) => Promise<unknown>, cancel: () => void) => {
      const credentials = navigator.credentials as CredentialsContainer | undefined
      const PKC = window.PublicKeyCredential as (typeof PublicKeyCredential & Record<string, unknown>) | undefined
      if (!credentials || !PKC) return

      // A bound function displays as `function () { [native code] }`, like Chrome's native functions.
      const native = <T extends (...args: never[]) => unknown>(name: string, fn: T): T => {
        const bound = fn.bind(null) as T
        Object.defineProperty(bound, 'name', { value: name })
        return bound
      }
      // Replaces a method where it lives, keeping writable / enumerable / configurable.
      const replace = (target: object, key: string, make: (original: (...args: unknown[]) => unknown) => (...args: never[]) => unknown): void => {
        const d = Object.getOwnPropertyDescriptor(target, key)
        if (!d || typeof d.value !== 'function') return
        Object.defineProperty(target, key, { ...d, value: native(key, make(d.value as (...args: unknown[]) => unknown)) })
      }

      const toBase64Url = (bytes: Uint8Array): string => {
        let bin = ''
        for (let i = 0; i < bytes.length; i += 0x8000) bin += String.fromCharCode(...bytes.subarray(i, i + 0x8000))
        return btoa(bin).replace(/\+/g, '-').replace(/\//g, '_').replace(/=+$/, '')
      }
      // Plain data: BufferSource -> base64url string, functions and undefined dropped.
      const toPlain = (v: unknown, depth = 0): unknown => {
        if (depth > 20) throw new TypeError('The options are too deeply nested.')
        if (v === null || typeof v === 'string' || typeof v === 'number' || typeof v === 'boolean') return v
        if (typeof v !== 'object') return undefined
        const tag = Object.prototype.toString.call(v)
        if (tag === '[object ArrayBuffer]') return toBase64Url(new Uint8Array(v as ArrayBuffer))
        if (ArrayBuffer.isView(v)) return toBase64Url(new Uint8Array(v.buffer, v.byteOffset, v.byteLength))
        if (Array.isArray(v)) return v.map((x) => toPlain(x, depth + 1)).filter((x) => x !== undefined)
        const out: Record<string, unknown> = {}
        for (const key of Object.keys(v)) {
          const x = toPlain((v as Record<string, unknown>)[key], depth + 1)
          if (x !== undefined) out[key] = x
        }
        return out
      }

      const fromBase64Url = (s: string): ArrayBuffer => {
        const bin = atob(s.replace(/-/g, '+').replace(/_/g, '/'))
        const out = new Uint8Array(bin.length)
        for (let i = 0; i < bin.length; i++) out[i] = bin.charCodeAt(i)
        return out.buffer
      }
      // Own, non-enumerable properties on an object whose prototype is the native one: instanceof works, the native getters
      // (which need internal slots) are shadowed, and Object.keys / JSON.stringify show nothing, like the real ones.
      const define = (target: object, props: Record<string, unknown>): void => {
        for (const [key, value] of Object.entries(props)) {
          Object.defineProperty(target, key, { value, enumerable: false, configurable: true, writable: false })
        }
      }
      const protoOf = (name: string, fallback: object): object => {
        const ctor = (window as unknown as Record<string, { prototype?: object } | undefined>)[name]
        return ctor?.prototype ?? fallback
      }
      // The credential from main (plain base64url data) as an object that behaves like a PublicKeyCredential.
      const buildCredential = (json: CredentialJson): unknown => {
        const copy = (): CredentialJson => JSON.parse(JSON.stringify(json)) as CredentialJson
        const r = json.response
        const isCreate = 'attestationObject' in r
        const response = Object.create(protoOf(isCreate ? 'AuthenticatorAttestationResponse' : 'AuthenticatorAssertionResponse', Object.prototype)) as object
        const common = { clientDataJSON: fromBase64Url(r.clientDataJSON), authenticatorData: fromBase64Url(r.authenticatorData) }
        if (isCreate) {
          const a = r as AttestationResponseJson
          const publicKey = a.publicKey === undefined ? null : fromBase64Url(a.publicKey)
          define(response, {
            clientDataJSON: common.clientDataJSON,
            attestationObject: fromBase64Url(a.attestationObject),
            getTransports: native('getTransports', () => [...a.transports]),
            getAuthenticatorData: native('getAuthenticatorData', () => common.authenticatorData),
            getPublicKey: native('getPublicKey', () => publicKey),
            getPublicKeyAlgorithm: native('getPublicKeyAlgorithm', () => a.publicKeyAlgorithm ?? -1)
          })
        } else {
          const a = r as AssertionResponseJson
          define(response, {
            clientDataJSON: common.clientDataJSON,
            authenticatorData: common.authenticatorData,
            signature: fromBase64Url(a.signature),
            userHandle: a.userHandle === undefined ? null : fromBase64Url(a.userHandle)
          })
        }
        const credential = Object.create(PKC.prototype) as object
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

      const NOT_ALLOWED = 'The operation either timed out or was not allowed. See: https://www.w3.org/TR/webauthn-2/#sctn-privacy-considerations-client.'
      const failure = (name: string, message: string): unknown => (name === 'TypeError' ? new TypeError(message) : new DOMException(message, name))

      const intercept =
        (type: 'get' | 'create') =>
        (original: (...args: unknown[]) => unknown) =>
        (...args: unknown[]): Promise<unknown> => {
          const options = args[0] as (CredentialRequestOptions & CredentialCreationOptions) | null | undefined
          if (options === null || typeof options !== 'object' || options.publicKey === undefined) {
            return original.apply(credentials, args) as Promise<unknown>
          }
          return new Promise((resolve, reject) => {
            const signal = options.signal
            if (signal?.aborted) return reject(signal.reason)
            let done = false
            let asked = false
            const settle = (fn: (v: unknown) => void, v: unknown): void => {
              if (done) return
              done = true
              signal?.removeEventListener('abort', onAbort)
              fn(v)
            }
            const onAbort = (): void => {
              const pending = asked && !done
              settle(reject, signal?.reason)
              if (pending) cancel()
            }
            signal?.addEventListener('abort', onAbort)
            // Conditional UI: pending until aborted, nothing to ask.
            if (type === 'get' && (options as CredentialRequestOptions).mediation === 'conditional') return
            let payload: unknown
            try {
              payload = { type, mediation: (options as CredentialRequestOptions).mediation, publicKey: toPlain(options.publicKey) }
            } catch (e) {
              return settle(reject, e)
            }
            asked = true
            ask(payload).then(
              (answer) => {
                const a = answer as { error?: { name?: string; message?: string }; credential?: unknown } | null
                if (a?.error) settle(reject, failure(String(a.error.name), String(a.error.message)))
                else if (a?.credential) {
                  try {
                    settle(resolve, buildCredential(a.credential as CredentialJson))
                  } catch {
                    settle(reject, failure('NotAllowedError', NOT_ALLOWED))
                  }
                } else settle(reject, failure('NotAllowedError', NOT_ALLOWED))
              },
              () => settle(reject, failure('NotAllowedError', NOT_ALLOWED))
            )
          })
        }

      const proto = Object.getPrototypeOf(credentials) as object
      replace(proto, 'get', intercept('get'))
      replace(proto, 'create', intercept('create'))

      // No authenticator here: sites must not offer passkeys as the first choice.
      replace(PKC, 'isUserVerifyingPlatformAuthenticatorAvailable', () => () => Promise.resolve(false))
      replace(PKC, 'isConditionalMediationAvailable', () => () => Promise.resolve(false))
      const KNOWN = [
        'conditionalCreate',
        'conditionalGet',
        'hybridTransport',
        'passkeyPlatformAuthenticator',
        'relatedOrigins',
        'userVerifyingPlatformAuthenticator',
        'extension:appid',
        'extension:credBlob',
        'extension:credProps',
        'extension:hmacCreateSecret',
        'extension:largeBlob',
        'extension:minPinLength',
        'extension:prf',
        'signalAllAcceptedCredentials',
        'signalCurrentUserDetails',
        'signalUnknownCredential'
      ]
      replace(PKC, 'getClientCapabilities', (original) => () => {
        const allFalse = (keys: string[]): Record<string, boolean> => Object.fromEntries(keys.map((k) => [k, false]))
        try {
          return Promise.resolve(original.call(PKC)).then(
            (caps) => allFalse(Object.keys((caps as object) ?? {})),
            () => allFalse(KNOWN)
          )
        } catch {
          return Promise.resolve(allFalse(KNOWN))
        }
      })
    },
    args: [request, abort]
  })
} catch {
  // frame without context isolation: nothing to do, the page keeps Electron's behavior
}

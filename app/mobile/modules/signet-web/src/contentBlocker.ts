import { useEffect, useRef, type RefObject } from 'react'
import { findNodeHandle } from 'react-native'
import { SignetWeb, type CompiledList } from './SignetWebModule'

/** True if native blocking exists (compiled iOS build); false in Expo Go and on Android, where only the JS layer acts. */
export const nativeBlockingAvailable = SignetWeb !== null

let ready: Promise<CompiledList[]> | null = null

/** Compiles the bundled lists once per launch (instant afterwards: they stay in the WebKit store). */
export function prepareContentBlocker(): Promise<CompiledList[]> {
  if (!SignetWeb) return Promise.resolve([])
  ready ??= SignetWeb.compileBundledRules()
    .then((lists) => {
      if (__DEV__)
        console.log(`[signet-web] ${lists.reduce((n, l) => n + l.count, 0)} rules compiled: ${lists.map((l) => `${l.identifier}=${l.count}`).join(', ')}`)
      return lists
    })
    .catch((e) => {
      ready = null
      if (__DEV__) console.warn('[signet-web] compile failed', e)
      return []
    })
  return ready
}

/** Attaches (or detaches) native blocking to the WebView with this view tag. Returns true if the WKWebView was found. */
export async function attachContentBlocker(viewTag: number, enabled: boolean): Promise<boolean> {
  if (!SignetWeb) return false
  const lists = await prepareContentBlocker()
  if (!lists.length) return false
  const ids = lists.map((l) => l.identifier)
  const ok = enabled ? await SignetWeb.attachRules(viewTag, ids) : await SignetWeb.detachRules(viewTag, ids)
  if (__DEV__) console.log(`[signet-web] ${enabled ? 'attach' : 'detach'} tag=${viewTag} -> ${ok ? 'ok' : 'WKWebView not found'}`)
  return ok
}

const RETRY_MS = [0, 150, 400, 1000, 2500]

/**
 * Hooks native blocking onto a react-native-webview `WebView`. The WKWebView is created when the view enters the window:
 * we retry a few times. A page already loading is not affected, the following ones are.
 * Does nothing outside a compiled iOS build.
 */
export function useContentBlocker(ref: RefObject<unknown>, enabled: boolean): void {
  const attached = useRef(false)
  useEffect(() => {
    if (!SignetWeb || (!enabled && !attached.current)) return
    let cancelled = false
    const timers: ReturnType<typeof setTimeout>[] = []
    const attempt = (i: number) => {
      if (cancelled) return
      const tag = ref.current ? findNodeHandle(ref.current as never) : null
      const next = () => {
        if (i + 1 < RETRY_MS.length) timers.push(setTimeout(() => attempt(i + 1), RETRY_MS[i + 1]))
      }
      if (tag == null) return next()
      attachContentBlocker(tag, enabled)
        .then((ok) => {
          if (ok) attached.current = enabled
          else next()
        })
        .catch(next)
    }
    timers.push(setTimeout(() => attempt(0), RETRY_MS[0]))
    return () => {
      cancelled = true
      timers.forEach(clearTimeout)
    }
  }, [ref, enabled])
}

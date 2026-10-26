// `localhost` addresses opened on the phone: when a computer is active, they go through it.
import { Platform, findNodeHandle } from 'react-native'
import { useEffect, type RefObject } from 'react'
import { isLoopbackUrl, toLinkLocalhost } from '@signet/shared/link/loopback'
import type { DevServer } from '@signet/shared/link/protocol'
import { activeLinkIds, decideLocalhost } from '@signet/shared/mobile/link'
import { SignetLink } from '../../../modules/signet-link'
import { t } from '@/lib/i18n'
import { useStore } from '@/state'
import { chooseLocalhostComputer, relayLocalhostPort } from './manager'
import { chooseComputer, useLinkRuntime } from './runtime'

const activeNow = (): string[] => {
  const st = useStore.getState()
  return activeLinkIds(st.links, useLinkRuntime.getState().byLink, st.settings.linkEnabled)
}

const nameOf = (linkId: string): string => useStore.getState().links.find((l) => l.linkId === linkId)?.desktop.name ?? ''

/** Asks to pick among the active computers; `null` if the user cancels. */
export function askActiveComputer(title: string): Promise<string | null> {
  return chooseComputer(
    title,
    activeNow().map((linkId) => ({ linkId, name: nameOf(linkId) }))
  )
}

/**
 * Address to open for what the user typed or tapped. Outside `localhost`, or with no active computer: unchanged.
 * Otherwise it is mapped back to `localhost`; with several computers, a window asks which one (`null` = cancelled).
 */
export async function resolveLocalhostUrl(url: string): Promise<string | null> {
  if (!isLoopbackUrl(url)) return url
  const d = decideLocalhost(url, activeNow(), useLinkRuntime.getState().chosen)
  if (d.kind === 'none') return url
  if (d.kind === 'route') {
    await relayLocalhostPort(d.url)
    return d.url
  }
  const linkId = await askActiveComputer(t('link.chooseForLocalhost'))
  if (!linkId) return null
  await chooseLocalhostComputer(linkId)
  await relayLocalhostPort(d.url)
  return d.url
}

/** Opens a computer's dev server: `localhost` now goes through it. Returns the address to open. */
export async function openDevServerUrl(linkId: string, server: DevServer): Promise<string> {
  await chooseLocalhostComputer(linkId)
  const url = toLinkLocalhost(server.url)
  await relayLocalhostPort(url)
  return url
}

const RETRY_MS = [0, 150, 400, 1000, 2500]

/**
 * Private tab: its WebView has its own data store, the `localhost` relay must be applied there (iOS).
 * Same principle as `useContentBlocker`: the native view is created when the view enters the window, so we retry a few times.
 */
export function useLocalhostAttach(ref: RefObject<unknown>, isPrivate: boolean): void {
  useEffect(() => {
    const native = SignetLink
    if (!native || Platform.OS !== 'ios' || !isPrivate) return
    let cancelled = false
    const timers: ReturnType<typeof setTimeout>[] = []
    const attempt = (i: number) => {
      if (cancelled) return
      const next = () => {
        if (i + 1 < RETRY_MS.length) timers.push(setTimeout(() => attempt(i + 1), RETRY_MS[i + 1]))
      }
      const tag = ref.current ? findNodeHandle(ref.current as never) : null
      if (tag == null) return next()
      native
        .attachToWebView(tag)
        .then((ok) => {
          if (!ok) next()
        })
        .catch(next)
    }
    timers.push(setTimeout(() => attempt(0), RETRY_MS[0]))
    return () => {
      cancelled = true
      timers.forEach(clearTimeout)
    }
  }, [ref, isPrivate])
}

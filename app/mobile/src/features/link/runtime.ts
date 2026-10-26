// Link state that is not persisted: open connections, dev servers seen, computer chosen for localhost.
import { create } from 'zustand'
import type { DevServer } from '@signet/shared/link/protocol'

export interface LinkRuntime {
  /** The control connection is open (after `welcome`). */
  connected: boolean
  /** A first connection is in progress (retries after a failure stay silent). */
  connecting: boolean
  servers: DevServer[]
  /** All ports listening on the computer (pages and APIs): the phone relays them from its own localhost. */
  ports: number[]
}

export const EMPTY_RUNTIME: LinkRuntime = { connected: false, connecting: false, servers: [], ports: [] }

interface RuntimeState {
  byLink: Record<string, LinkRuntime | undefined>
  /** Computer chosen for `localhost` when several are active (kept as long as it stays active). */
  chosen: string | null
  /** Computer that actually receives `localhost` right now. */
  routeLinkId: string | null
  /** `localhost` ports opened by hand on this phone, relayed too (even if the computer does not announce them). */
  extraPorts: number[]
  /** The WebView can send `localhost` to the computer (iOS 17+, recent Android); `null` while unknown. */
  routeSupported: boolean | null
  /** The native module is present (false in Expo Go). */
  nativeAvailable: boolean
}

export const useLinkRuntime = create<RuntimeState>(() => ({
  byLink: {},
  chosen: null,
  routeLinkId: null,
  extraPorts: [],
  routeSupported: null,
  nativeAvailable: false
}))

export function patchRuntime(linkId: string, patch: Partial<LinkRuntime>): void {
  useLinkRuntime.setState((s) => ({ byLink: { ...s.byLink, [linkId]: { ...(s.byLink[linkId] ?? EMPTY_RUNTIME), ...patch } } }))
}

export function clearRuntime(linkId: string): void {
  useLinkRuntime.setState((s) => {
    const { [linkId]: _gone, ...rest } = s.byLink
    return { byLink: rest, chosen: s.chosen === linkId ? null : s.chosen }
  })
}

// --- "Which computer?" window ---

export interface ComputerChoice {
  title: string
  options: { linkId: string; name: string }[]
  /** First winning answer; `null` = cancelled. */
  settle: (linkId: string | null) => void
}

export const useChoice = create<{ current: ComputerChoice | null }>(() => ({ current: null }))

/** Asks the user to choose a computer; resolves with its `linkId`, or `null` if cancelled. */
export function chooseComputer(title: string, options: { linkId: string; name: string }[]): Promise<string | null> {
  return new Promise((resolve) => {
    let done = false
    const settle = (linkId: string | null) => {
      if (done) return
      done = true
      useChoice.setState({ current: null })
      resolve(linkId)
    }
    useChoice.getState().current?.settle(null)
    useChoice.setState({ current: { title, options, settle } })
  })
}

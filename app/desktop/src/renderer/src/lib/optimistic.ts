import { useSyncExternalStore } from 'react'

// Optimistic selection: the chosen tab or space is shown as soon as the command is sent,
// without waiting for the main process's state. It drops at the first update received after the response,
// or after a short delay if the main process refused (no state arrives then).

interface Pending {
  tabId?: string
  spaceId?: string
}

const SETTLE_DELAY = 400

let pending: Pending = {}
let seq = 0
let landed = false
let timer: ReturnType<typeof setTimeout> | undefined
const listeners = new Set<() => void>()

const set = (next: Pending) => {
  pending = next
  listeners.forEach((fn) => fn())
}

const drop = (mine: number) => {
  if (mine !== seq) return
  clearTimeout(timer)
  landed = false
  // Nothing pending: no notification, hence no useless render.
  if (pending.tabId === undefined && pending.spaceId === undefined) return
  set({})
}

function run(patch: Pending, send: Promise<unknown>) {
  const mine = ++seq
  landed = false
  clearTimeout(timer)
  set({ ...pending, ...patch })
  send.then(
    () => {
      if (mine !== seq) return
      landed = true
      timer = setTimeout(() => drop(mine), SETTLE_DELAY)
    },
    () => drop(mine)
  )
}

/** To call on each state received: the optimistic value is stale as soon as the main process has responded. */
export function settleOptimistic(): void {
  if (landed) drop(seq)
}

/** Hint from the main process: a space is chosen outside this window (overlay popover). Drops at the next state. */
export function adoptPendingSpace(spaceId: string): void {
  const mine = ++seq
  landed = true // the next state received makes it stale
  clearTimeout(timer)
  set({ spaceId, tabId: undefined })
  timer = setTimeout(() => drop(mine), 1000) // the main process refused: no state arrives
}

export function activateTab(tabId: string): void {
  run({ tabId }, window.signet.activateTab(tabId))
}

export function switchSpace(spaceId: string): void {
  // Changing space restores that space's active tab: we forget the tab choice.
  run({ spaceId, tabId: undefined }, window.signet.switchSpace(spaceId))
}

const subscribe = (fn: () => void) => {
  listeners.add(fn)
  return () => void listeners.delete(fn)
}

export function useOptimistic(): Pending {
  return useSyncExternalStore(subscribe, () => pending)
}

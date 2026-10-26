import { readFileSync } from 'node:fs'
import { join } from 'node:path'
import { describe, expect, it } from 'vitest'
import { IPC, PAGE_IPC, PIP_IPC } from '@shared/ipc/contract'

/** Names of the methods declared in `interface SignetApi` (src/shared/types/api.ts). */
function signetApiMethods(): string[] {
  const src = readFileSync(join(__dirname, '../../shared/types/api.ts'), 'utf8')
  const body = src.slice(src.indexOf('export interface SignetApi {'), src.indexOf('declare global'))
  return [...body.matchAll(/^ {2}(\w+)\(/gm)].map((m) => m[1])
}

describe('IPC contract', () => {
  it('every SignetApi method has an entry, and vice versa', () => {
    expect(Object.keys(IPC).sort()).toEqual(signetApiMethods().sort())
  })

  it('names starting with `on…` are subscriptions, the others are calls', () => {
    for (const [name, def] of Object.entries(IPC)) expect(def.kind, name).toBe(/^on[A-Z]/.test(name) ? 'event' : 'invoke')
  })

  it('call channels are unique, and so are event channels', () => {
    for (const kind of ['invoke', 'event'] as const) {
      const channels = Object.values(IPC)
        .filter((d) => d.kind === kind)
        .map((d) => d.channel)
      expect(
        channels.filter((c, i) => channels.indexOf(c) !== i),
        kind
      ).toEqual([])
    }
  })

  it('no channel is shared between the interface, the mini player and web pages', () => {
    const calls = [...Object.values(IPC).filter((d) => d.kind === 'invoke'), ...Object.values(PIP_IPC)].map((d) => d.channel)
    const all = [...calls, ...Object.keys(PAGE_IPC)]
    expect(all.filter((c, i) => all.indexOf(c) !== i)).toEqual([])
  })

  it('a call declares one schema per argument', () => {
    for (const [name, def] of Object.entries(IPC)) if (def.kind === 'invoke') expect(Array.isArray(def.args), name).toBe(true)
  })
})

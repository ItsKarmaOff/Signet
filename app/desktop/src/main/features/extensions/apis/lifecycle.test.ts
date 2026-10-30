import { mkdtempSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { describe, expect, it, vi } from 'vitest'
import { ContextMenus } from './contextmenus'
import { DnrStore } from './dnr'
import { PersistedData } from './lifecycle'
import { PermissionGrants } from './permissions'
import { AlarmScheduler } from './schedule'

const A = 'a'.repeat(32)
const B = 'b'.repeat(32)

function make() {
  const dir = mkdtempSync(join(tmpdir(), 'signet-life-'))
  const emitter = { emit: vi.fn(), extensionsWith: vi.fn(() => []) }
  const menus = new ContextMenus(emitter)
  menus.persist(join(dir, 'menus.json'))
  const grants = new PermissionGrants(join(dir, 'grants.json'), emitter)
  const dnr = new DnrStore(join(dir, 'dnr.json'), () => {})
  const alarms = new AlarmScheduler({ fire: vi.fn(), changed: vi.fn() }, {})
  const data = new PersistedData(() => [
    { knownIds: () => menus.knownIds(), forget: (id) => menus.unload(id) },
    { knownIds: () => grants.knownIds(), forget: (id) => grants.drop(id) },
    { knownIds: () => dnr.knownIds(), forget: (id) => dnr.remove(id) },
    { knownIds: () => alarms.knownIds(), forget: (id) => alarms.remove(id) }
  ])
  const fill = (id: string) => {
    menus.create(id, { id: 'm', title: 'M' })
    dnr.setEnabledRulesets(id, '1', ['r'])
    dnr.setDisplayActionCount(id, true)
    alarms.create(id, 'tick', { periodInMinutes: 1 })
  }
  const held = (id: string) => ({
    menus: menus.has(id),
    dnr: dnr.displayActionCount(id),
    rulesets: dnr.enabledRulesets(id, '1'),
    alarms: alarms.getAll(id).length
  })
  return { menus, grants, dnr, alarms, data, fill, held }
}

describe('unload versus uninstall', () => {
  it('an unload (runtime cleanup only) keeps menus, rules, rulesets, count and alarms, and they are back after the reload', () => {
    const { dnr, alarms, fill, held } = make()
    fill(A)
    // What the unload handler does for the stores.
    dnr.clearSession(A)
    alarms.suspend(A)
    expect(held(A)).toEqual({ menus: true, dnr: true, rulesets: ['r'], alarms: 1 })
    alarms.activate(A)
    expect(held(A).alarms).toBe(1)
    alarms.stop()
  })

  it('unload drops session rules only', () => {
    const { dnr } = make()
    dnr.update('session', A, { addRules: [{ id: 1, action: { type: 'block' }, condition: { urlFilter: 'x' } }] })
    dnr.update('dynamic', A, { addRules: [{ id: 2, action: { type: 'block' }, condition: { urlFilter: 'y' } }] })
    dnr.clearSession(A)
    expect(dnr.get('session', A, undefined)).toEqual([])
    expect(dnr.get('dynamic', A, undefined)).toHaveLength(1)
  })

  it('forget (uninstall) drops everything for that extension and only that one', () => {
    const { data, fill, held, grants } = make()
    fill(A)
    fill(B)
    data.forget(A)
    expect(held(A)).toEqual({ menus: false, dnr: false, rulesets: undefined, alarms: 0 })
    expect(held(B).menus).toBe(true)
    expect(grants.knownIds()).toEqual([])
  })
})

describe('startup garbage collection', () => {
  it('drops unknown ids and keeps installed ones', () => {
    const { data, fill, held } = make()
    fill(A)
    fill(B)
    expect(data.collect(new Set([A]))).toEqual([B])
    expect(held(A).menus).toBe(true)
    expect(held(B)).toEqual({ menus: false, dnr: false, rulesets: undefined, alarms: 0 })
    expect(data.collect(new Set([A]))).toEqual([])
  })
})

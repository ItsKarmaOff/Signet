import { mkdtempSync, rmSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type { Session } from 'electron'
import { ChromeSettings, type SettingDef } from './settings'
import { ApiError, type ExtensionEmitter } from './types'
import { parseProxyConfig, SYSTEM_PROXY } from './proxyconfig'

let dir: string
beforeEach(() => {
  dir = mkdtempSync(join(tmpdir(), 'signet-settings-'))
})
afterEach(() => rmSync(dir, { recursive: true, force: true, maxRetries: 5, retryDelay: 50 }))

const ses = {} as Session
const applied = vi.fn()
const def: SettingDef = {
  key: 'proxy.settings',
  permission: 'proxy',
  defaultValue: SYSTEM_PROXY,
  parse: parseProxyConfig,
  apply: (s, v) => applied(s, v)
}
const direct = { mode: 'direct' }
const auto = { mode: 'auto_detect' }

function make(loaded: string[] = ['A', 'B']) {
  const emit = vi.fn()
  const emitter: ExtensionEmitter = { emit, extensionsWith: () => loaded }
  const settings = new ChromeSettings(join(dir, 's.json'), [def], emitter, () => [ses])
  return { settings, emitter: { emit } }
}
const tick = () => new Promise((r) => setTimeout(r, 0))

describe('ChromeSettings', () => {
  it('reports the default and who may control it', () => {
    const { settings } = make()
    expect(settings.details('proxy.settings', 'A')).toEqual({ value: SYSTEM_PROXY, levelOfControl: 'controllable_by_this_extension', incognitoSpecific: false })
  })

  it('set: the setter controls, the others see it as controlled by another extension', async () => {
    const { settings } = make()
    settings.set('proxy.settings', 'A', { value: direct })
    expect(settings.details('proxy.settings', 'A')).toMatchObject({ value: direct, levelOfControl: 'controlled_by_this_extension' })
    expect(settings.details('proxy.settings', 'B')).toMatchObject({ value: direct, levelOfControl: 'controlled_by_other_extensions' })
    await tick()
    expect(applied).toHaveBeenLastCalledWith(ses, direct)
  })

  it('the last setter wins, and only the controller can clear', async () => {
    const { settings } = make()
    settings.set('proxy.settings', 'A', { value: direct })
    settings.set('proxy.settings', 'B', { value: auto })
    expect(settings.controllerOf('proxy.settings')).toBe('B')
    settings.clear('proxy.settings', 'A')
    expect(settings.controllerOf('proxy.settings')).toBe('B')
    settings.clear('proxy.settings', 'B')
    expect(settings.controllerOf('proxy.settings')).toBeUndefined()
    await tick()
    expect(applied).toHaveBeenLastCalledWith(ses, SYSTEM_PROXY)
  })

  it('rejects an invalid value and leaves the state alone', () => {
    const { settings } = make()
    expect(() => settings.set('proxy.settings', 'A', { value: { mode: 'x' } })).toThrow(ApiError)
    expect(settings.controllerOf('proxy.settings')).toBeUndefined()
  })

  it('onChange: sent to extensions whose view changed, waking only the ones that gained or lost control', () => {
    const { settings, emitter } = make()
    settings.set('proxy.settings', 'A', { value: direct })
    expect(emitter.emit).toHaveBeenCalledWith(
      'A',
      'proxy.settings.onChange',
      [{ value: direct, levelOfControl: 'controlled_by_this_extension', incognitoSpecific: false }],
      true
    )
    expect(emitter.emit).toHaveBeenCalledWith(
      'B',
      'proxy.settings.onChange',
      [{ value: direct, levelOfControl: 'controlled_by_other_extensions', incognitoSpecific: false }],
      false
    )
    emitter.emit.mockClear()
    settings.set('proxy.settings', 'A', { value: direct }) // same effective value and level: nothing
    expect(emitter.emit).not.toHaveBeenCalled()
  })

  it('release: an unloaded extension gives everything back', () => {
    const { settings } = make()
    settings.set('proxy.settings', 'A', { value: direct })
    settings.release('A')
    expect(settings.controllerOf('proxy.settings')).toBeUndefined()
    expect(settings.effective('proxy.settings')).toEqual(SYSTEM_PROXY)
  })

  it('persists the controller and reapplies when it loads', async () => {
    const first = make()
    first.settings.set('proxy.settings', 'A', { value: direct })
    expect(first.settings.flush()).toBe(true)
    await tick() // the first instance's own apply
    const second = make()
    expect(second.settings.controllerOf('proxy.settings')).toBe('A')
    applied.mockClear()
    second.settings.reapply(ses, 'B')
    await tick()
    expect(applied).not.toHaveBeenCalled()
    second.settings.reapply(ses, 'A')
    await tick()
    expect(applied).toHaveBeenCalledWith(ses, direct)
  })
})

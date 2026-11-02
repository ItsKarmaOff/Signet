import { describe, expect, it, vi } from 'vitest'
import { parseSuggestedKey, type CommandPlatform } from '@shared/extension-commands'
import { signetAccelKeys } from '@shared/shortcuts'
import { COMMAND_EVENT, ExtensionCommands } from './commands'
import type { ExtensionEmitter } from './types'

const manifest = (commands: Record<string, unknown>) => ({ commands })

function setup(platform: CommandPlatform = 'win32', overrides?: Record<string, string | null>) {
  const emitter = { emit: vi.fn(), extensionsWith: vi.fn(() => []) } satisfies ExtensionEmitter
  const log = vi.fn()
  const runAction = vi.fn()
  const state = { overrides }
  const commands = new ExtensionCommands({
    platform,
    emitter,
    signetKeys: () => signetAccelKeys(state.overrides, platform === 'darwin'),
    localize: (_id, text) => (text === '__MSG_d__' ? 'Localized' : text),
    runAction,
    activeTab: (target) => (target ? { id: 7 } : undefined),
    log
  })
  return { commands, emitter, log, runAction, state }
}

describe('ExtensionCommands', () => {
  it('getAll: name, localized description, shortcut or empty', () => {
    const { commands } = setup('darwin')
    commands.register(
      'a',
      'A',
      manifest({
        _execute_action: { suggested_key: { default: 'Alt+Shift+D' } },
        zap: { description: '__MSG_d__' },
        go: { description: 'Go', suggested_key: { default: 'Ctrl+Shift+Y' } }
      })
    )
    expect(commands.listFor('a')).toEqual([
      { name: '_execute_action', description: '', shortcut: '⌥⇧D' },
      { name: 'zap', description: 'Localized', shortcut: '' },
      { name: 'go', description: 'Go', shortcut: '⇧⌘Y' }
    ])
    expect(commands.listFor('unknown')).toEqual([])
  })

  it('a loser stays listed with an empty shortcut and one dev line reports the clash, once', () => {
    const { commands, log } = setup()
    commands.register('a', 'A', manifest({ go: { description: 'Go', suggested_key: { default: 'Ctrl+Shift+Y' } } }))
    commands.register('b', 'B', manifest({ go: { description: 'Go', suggested_key: { default: 'Ctrl+Shift+Y' } } }))
    expect(commands.listFor('b')).toEqual([{ name: 'go', description: 'Go', shortcut: '' }])
    commands.listFor('b')
    commands.bindings()
    expect(log).toHaveBeenCalledTimes(1)
    expect(log).toHaveBeenCalledWith('[extension-commands] b go Ctrl+Shift+Y taken by a go')
    expect(commands.bindings()).toEqual([{ accel: 'Ctrl+Shift+Y', extensionId: 'a', name: 'go' }])
  })

  it('re-evaluates when extensions unload or Signet shortcuts change', () => {
    const { commands, state } = setup()
    commands.register('a', 'A', manifest({ go: { description: 'Go', suggested_key: { default: 'Alt+Shift+Q' } } }))
    commands.register('b', 'B', manifest({ go: { description: 'Go', suggested_key: { default: 'Alt+Shift+Q' } } }))
    expect(commands.bindings().map((b) => b.extensionId)).toEqual(['a'])
    const before = commands.key()
    commands.unregister('a')
    expect(commands.bindings().map((b) => b.extensionId)).toEqual(['b'])
    expect(commands.key()).not.toBe(before)
    // The user gives that shortcut to a Signet command: the extension loses it.
    state.overrides = { newTab: 'Alt+Shift+Q' }
    expect(commands.bindings()).toEqual([])
    expect(commands.listFor('b')[0].shortcut).toBe('')
  })

  it('a reloaded extension keeps its place in line', () => {
    const { commands } = setup()
    const cmd = { go: { description: 'Go', suggested_key: { default: 'Alt+Shift+Q' } } }
    commands.register('a', 'A', manifest(cmd))
    commands.register('b', 'B', manifest(cmd))
    commands.register('a', 'A', manifest(cmd))
    expect(commands.bindings().map((b) => b.extensionId)).toEqual(['a'])
  })

  it('lists every extension for Settings', () => {
    const { commands } = setup()
    commands.register('a', 'A', manifest({ one: { description: 'One' } }))
    commands.register('b', 'B', {})
    expect([...commands.listAll().keys()]).toEqual(['a'])
  })

  it('platform shortcuts are resolved per platform', () => {
    const win = setup('win32').commands
    const mac = setup('darwin').commands
    const m = manifest({ go: { description: 'Go', suggested_key: { default: 'Ctrl+Shift+Y', mac: 'MacCtrl+Shift+Y' } } })
    win.register('a', 'A', m)
    mac.register('a', 'A', m)
    expect(win.bindings()[0].accel).toBe('Ctrl+Shift+Y')
    expect(mac.bindings()[0].accel).toBe('Ctrl+Shift+Y')
    expect(mac.listFor('a')[0].shortcut).toBe('⌃⇧Y')
    expect(parseSuggestedKey('Ctrl+Shift+Y', 'darwin')).toMatchObject({ cmd: true, ctrl: false })
  })

  describe('run', () => {
    it('_execute_* commands click the toolbar button instead of raising onCommand', () => {
      const { commands, emitter, runAction } = setup()
      const target = { tab: 1 }
      for (const name of ['_execute_action', '_execute_browser_action', '_execute_page_action']) commands.run('a', name, target)
      expect(runAction).toHaveBeenCalledTimes(3)
      expect(runAction).toHaveBeenCalledWith('a', target)
      expect(emitter.emit).not.toHaveBeenCalled()
    })

    it('other commands wake the worker with (name, tab)', () => {
      const { commands, emitter, runAction } = setup()
      commands.run('a', 'toggle', {})
      expect(emitter.emit).toHaveBeenCalledWith('a', COMMAND_EVENT, ['toggle', { id: 7 }], true)
      expect(runAction).not.toHaveBeenCalled()
    })

    it('without a tab only the name is sent', () => {
      const { commands, emitter } = setup()
      commands.run('a', 'toggle')
      expect(emitter.emit).toHaveBeenCalledWith('a', COMMAND_EVENT, ['toggle'], true)
    })
  })

  it('router handler answers for the calling extension', () => {
    const { commands } = setup()
    commands.register('a', 'A', manifest({ one: { description: 'One' } }))
    const handler = commands.methods()['commands.getAll']
    expect(handler({ extensionId: 'a' } as never)).toEqual([{ name: 'one', description: 'One', shortcut: '' }])
  })
})

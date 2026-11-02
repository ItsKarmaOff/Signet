import { describe, expect, it } from 'vitest'
import {
  displayShortcut,
  localeCandidates,
  localizeText,
  parseSuggestedKey,
  readCommands,
  resolveCommands,
  suggestedKeyFor,
  toAccelerator,
  type ManifestCommand,
  type Messages
} from './extension-commands'
import { accelKey, signetAccelKeys } from './shortcuts'

const acc = (text: string, platform: 'darwin' | 'win32' | 'linux') => {
  const p = parseSuggestedKey(text, platform)
  return p && toAccelerator(p)
}

describe('parseSuggestedKey', () => {
  it('maps Ctrl to Command on macOS and keeps it Control elsewhere', () => {
    expect(acc('Ctrl+Shift+Y', 'darwin')).toBe('Cmd+Shift+Y')
    expect(acc('Ctrl+Shift+Y', 'win32')).toBe('Ctrl+Shift+Y')
    expect(acc('Ctrl+Shift+Y', 'linux')).toBe('Ctrl+Shift+Y')
  })

  it('MacCtrl is the real Control key, and Command exists on macOS only', () => {
    expect(acc('MacCtrl+Shift+Y', 'darwin')).toBe('Ctrl+Shift+Y')
    expect(acc('Command+Shift+Y', 'darwin')).toBe('Cmd+Shift+Y')
    expect(acc('MacCtrl+Shift+Y', 'win32')).toBeNull()
    expect(acc('Command+Y', 'linux')).toBeNull()
  })

  it('reads the supported keys', () => {
    for (const k of ['A', 'Z', '0', '9', 'F1', 'F12']) expect(acc(`Alt+${k}`, 'win32')).toBe(`Alt+${k}`)
    expect(acc('Alt+Comma', 'win32')).toBe('Alt+,')
    expect(acc('Alt+Period', 'win32')).toBe('Alt+.')
    for (const k of ['Home', 'End', 'PageUp', 'PageDown', 'Space', 'Insert', 'Delete', 'Up', 'Down', 'Left', 'Right'])
      expect(acc(`Alt+${k}`, 'linux')).toBe(`Alt+${k}`)
    expect(acc('Alt+UpArrow', 'linux')).toBe('Alt+Up')
  })

  it('allows media keys without a modifier', () => {
    expect(acc('MediaPlayPause', 'win32')).toBe('MediaPlayPause')
    expect(acc('MediaPrevTrack', 'darwin')).toBe('MediaPreviousTrack')
    expect(acc('MediaNextTrack', 'linux')).toBe('MediaNextTrack')
    expect(acc('MediaStop', 'linux')).toBe('MediaStop')
  })

  it('rejects what Chrome rejects', () => {
    expect(acc('Shift+Y', 'win32')).toBeNull()
    expect(acc('Y', 'win32')).toBeNull()
    expect(acc('F5', 'win32')).toBeNull()
    expect(acc('Ctrl+F13', 'win32')).toBeNull()
    expect(acc('Ctrl+y', 'win32')).toBeNull()
    expect(acc('Ctrl+Ctrl+Y', 'win32')).toBeNull()
    expect(acc('Ctrl+Win+Y', 'win32')).toBeNull()
    expect(acc('Search+Y', 'linux')).toBeNull()
    expect(acc('', 'linux')).toBeNull()
    expect(parseSuggestedKey(42, 'linux')).toBeNull()
  })
})

describe('suggestedKeyFor', () => {
  const spec = { default: 'Ctrl+Shift+Y', mac: 'Command+Shift+U', windows: 'Alt+W', linux: 'Alt+L', chromeos: 'Alt+C' }
  it('prefers the platform entry, then default', () => {
    expect(suggestedKeyFor(spec, 'darwin')).toBe('Command+Shift+U')
    expect(suggestedKeyFor(spec, 'win32')).toBe('Alt+W')
    expect(suggestedKeyFor(spec, 'linux')).toBe('Alt+L')
    expect(suggestedKeyFor({ default: 'Ctrl+Y' }, 'linux')).toBe('Ctrl+Y')
    expect(suggestedKeyFor({ mac: 'Ctrl+Y' }, 'linux')).toBeUndefined()
    expect(suggestedKeyFor('Ctrl+Y', 'linux')).toBe('Ctrl+Y')
    expect(suggestedKeyFor(undefined, 'linux')).toBeUndefined()
  })
})

describe('displayShortcut', () => {
  it('uses symbols on macOS and words elsewhere', () => {
    const p = parseSuggestedKey('Ctrl+Shift+Y', 'darwin')!
    expect(displayShortcut(p, 'darwin')).toBe('⇧⌘Y')
    expect(displayShortcut(parseSuggestedKey('MacCtrl+Alt+Up', 'darwin')!, 'darwin')).toBe('⌃⌥↑')
    expect(displayShortcut(parseSuggestedKey('Ctrl+Shift+Y', 'win32')!, 'win32')).toBe('Ctrl+Shift+Y')
    expect(displayShortcut(parseSuggestedKey('Alt+Ctrl+Shift+Up', 'linux')!, 'linux')).toBe('Ctrl+Alt+Shift+Up')
  })
})

describe('readCommands', () => {
  it('lists commands in manifest order with their platform shortcut', () => {
    const manifest = {
      commands: {
        _execute_action: { suggested_key: { default: 'Alt+Shift+D' } },
        zap: { description: 'Zap' },
        toggle: { description: 'Toggle', suggested_key: { default: 'Ctrl+Shift+L', mac: 'Command+Shift+L' } }
      }
    }
    const mac = readCommands(manifest, 'darwin')
    expect(mac.map((c) => c.name)).toEqual(['_execute_action', 'zap', 'toggle'])
    expect(mac.map((c) => c.key && toAccelerator(c.key))).toEqual(['Alt+Shift+D', null, 'Cmd+Shift+L'])
    expect(readCommands(manifest, 'win32')[2].key).toMatchObject({ ctrl: true, key: 'L' })
  })

  it('keeps at most four suggested keys and ignores the rest', () => {
    const commands = Object.fromEntries(['A', 'B', 'C', 'D', 'E', 'F'].map((k) => [`c${k}`, { description: k, suggested_key: { default: `Alt+${k}` } }]))
    const keys = readCommands({ commands }, 'linux').map((c) => !!c.key)
    expect(keys).toEqual([true, true, true, true, false, false])
  })

  it('invalid shortcuts do not use up the limit; junk is tolerated', () => {
    const commands = { bad: { suggested_key: { default: 'Shift+X' } }, a: { suggested_key: { default: 'Alt+A' } } }
    expect(readCommands({ commands }, 'linux').map((c) => !!c.key)).toEqual([false, true])
    expect(readCommands(undefined, 'linux')).toEqual([])
    expect(readCommands({ commands: [] }, 'linux')).toEqual([])
    expect(readCommands({ commands: { x: 3 } }, 'linux')).toEqual([{ name: 'x', description: '', key: null }])
  })
})

describe('resolveCommands', () => {
  const set = (id: string, ...cmds: [string, string | null][]) => ({
    id,
    name: id,
    commands: cmds.map(([name, key]): ManifestCommand => ({ name, description: name, key: key ? parseSuggestedKey(key, 'win32') : null }))
  })

  it('gives a shortcut to the first extension that asks, the other keeps its command without one', () => {
    const { commands, clashes } = resolveCommands(
      [set('one', ['go', 'Ctrl+Shift+Y']), set('two', ['go', 'Ctrl+Shift+Y'], ['other', 'Alt+Q'])],
      new Map(),
      'win32'
    )
    expect(commands.map((c) => [c.extensionId, c.name, c.shortcut])).toEqual([
      ['one', 'go', 'Ctrl+Shift+Y'],
      ['two', 'go', ''],
      ['two', 'other', 'Alt+Q']
    ])
    expect(commands[1].accel).toBeNull()
    expect(clashes).toEqual([{ extensionId: 'two', command: 'go', accel: 'Ctrl+Shift+Y', owner: 'one go' }])
  })

  it("Signet's own shortcuts win, extras and user overrides included", () => {
    const signet = signetAccelKeys({ newTab: 'Alt+Shift+Q' }, false)
    // The user's override, default shortcuts and reserved ones all count.
    expect(signet.get(accelKey('Alt+Shift+Q', false)!)).toBe('newTab')
    expect(signet.has(accelKey('Ctrl+T', false)!)).toBe(false)
    const { commands, clashes } = resolveCommands(
      [set('one', ['a', 'Alt+Shift+Q'], ['b', 'Ctrl+W'], ['c', 'Ctrl+C'], ['d', 'Ctrl+Shift+T'], ['e', 'Alt+Shift+Z'])],
      signet,
      'win32'
    )
    expect(commands.map((c) => c.shortcut)).toEqual(['', '', '', '', 'Alt+Shift+Z'])
    expect(clashes.map((c) => c.command)).toEqual(['a', 'b', 'c', 'd'])
    expect(clashes.every((c) => c.owner === 'Signet')).toBe(true)
  })

  it('compares the extra shortcuts of a Signet command', () => {
    const signet = signetAccelKeys(undefined, true)
    const macSet = { id: 'x', name: 'x', commands: [{ name: 'n', description: '', key: parseSuggestedKey('Ctrl+Shift+S', 'darwin') }] }
    // altMac of tabLayout is Cmd+Shift+S.
    expect(resolveCommands([macSet], signet, 'darwin').commands[0].shortcut).toBe('')
  })
})

describe('localization', () => {
  const files: Record<string, Messages> = {
    fr: { Toggle: { message: 'Basculer $NAME$', placeholders: { name: { content: 'le mode' } } }, price: { message: '5$$' } },
    en: { toggle: { message: 'Toggle' }, only_en: { message: 'English only' } }
  }
  const load = (l: string) => files[l] ?? null

  it('builds the locale list', () => {
    expect(localeCandidates('fr-CA', 'en')).toEqual(['fr_CA', 'fr', 'en'])
    expect(localeCandidates('en-US', 'en')).toEqual(['en_US', 'en'])
    expect(localeCandidates('fr')).toEqual(['fr'])
  })

  it('resolves __MSG_key__ (case-insensitive) with placeholders and falls back to the default locale', () => {
    const c = ['fr', 'en']
    expect(localizeText('__MSG_toggle__', c, load)).toBe('Basculer le mode')
    expect(localizeText('__MSG_price__', c, load)).toBe('5$')
    expect(localizeText('__MSG_only_en__', c, load)).toBe('English only')
    expect(localizeText('__MSG_missing__', c, load)).toBe('')
  })

  it('leaves plain text alone', () => {
    expect(localizeText('Plain text', ['en'], load)).toBe('Plain text')
    expect(localizeText('', ['en'], load)).toBe('')
  })
})

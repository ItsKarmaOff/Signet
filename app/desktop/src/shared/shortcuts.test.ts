import { describe, expect, it } from 'vitest'
import {
  FIXED_SHORTCUTS,
  SHORTCUTS,
  SHORTCUT_GROUPS,
  accelKey,
  accelLabel,
  cleanOverrides,
  defaultAccel,
  parseAccel,
  reservedShortcuts,
  shortcutAccel,
  shortcutAccels,
  shortcutConflict,
  shortcutDef,
  shortcutLabel,
  takeOver,
  usableAccel
} from './shortcuts'

describe('table SHORTCUTS', () => {
  it('has unique identifiers', () => {
    const ids = SHORTCUTS.map((s) => s.id)
    expect(new Set(ids).size).toBe(ids.length)
  })

  it('uses only known groups and translatable labels', () => {
    for (const s of SHORTCUTS) {
      expect(SHORTCUT_GROUPS).toContain(s.group)
      expect(s.label).toMatch(/^settings\.shortcuts\.item\./)
    }
    for (const f of FIXED_SHORTCUTS) expect(SHORTCUT_GROUPS).toContain(f.group)
  })

  it('writes valid accelerators for both systems', () => {
    for (const s of SHORTCUTS) {
      for (const mac of [true, false]) {
        const all = [s.accel, s.mac, s.other, ...(s.alt ?? []), ...(s.altMac ?? [])].filter((a): a is string => !!a)
        for (const a of all) expect(parseAccel(a, mac), `${s.id}: ${a}`).not.toBeNull()
      }
    }
  })

  it('has no duplicate default between commands, per system', () => {
    for (const mac of [true, false]) {
      const seen = new Map<string, string>()
      for (const s of SHORTCUTS) {
        for (const a of shortcutAccels(s.id, undefined, mac)) {
          const key = accelKey(a, mac)!
          expect(seen.get(key), `${a} shared by ${s.id} and ${seen.get(key)} (mac=${mac})`).toBeUndefined()
          seen.set(key, s.id)
        }
      }
    }
  })

  it('does not conflict with any reserved shortcut', () => {
    for (const mac of [true, false]) {
      const reserved = new Set(reservedShortcuts(mac, !mac).map((r) => accelKey(r.accel, mac)))
      for (const s of SHORTCUTS) {
        for (const a of shortcutAccels(s.id, undefined, mac)) {
          expect(reserved.has(accelKey(a, mac)), `${s.id}: ${a} (mac=${mac})`).toBe(false)
        }
      }
    }
  })

  it('shortcutDef finds a command', () => {
    expect(shortcutDef('newTab')?.accel).toBe('CmdOrCtrl+T')
    expect(shortcutDef('nope')).toBeUndefined()
  })
})

describe('parseAccel', () => {
  it('resolves CmdOrCtrl according to the system', () => {
    expect(parseAccel('CmdOrCtrl+T', true)).toEqual({ cmd: true, ctrl: false, alt: false, shift: false, key: 'T' })
    expect(parseAccel('CmdOrCtrl+T', false)).toEqual({ cmd: false, ctrl: true, alt: false, shift: false, key: 'T' })
  })

  it('accepts modifier aliases regardless of case', () => {
    expect(parseAccel('command+option+shift+a', true)).toEqual({ cmd: true, ctrl: false, alt: true, shift: true, key: 'A' })
    expect(parseAccel('Control+Super+F5', false)).toMatchObject({ ctrl: true, cmd: true, key: 'F5' })
  })

  it('normalizes named keys', () => {
    expect(parseAccel('Ctrl+enter', true)?.key).toBe('Return')
    expect(parseAccel('Ctrl+esc', true)?.key).toBe('Escape')
    expect(parseAccel('Ctrl+pageup', true)?.key).toBe('PageUp')
    expect(parseAccel('Ctrl+F24', true)?.key).toBe('F24')
  })

  it('rejects invalid accelerators', () => {
    for (const bad of ['', '+', 'Ctrl+', '+T', 'Ctrl++', 'Foo+T', 'Ctrl+F25', 'Ctrl+F0', 'Ctrl+AB', 'Ctrl+é', 'Ctrl+T+']) {
      expect(parseAccel(bad, true), bad).toBeNull()
    }
    expect(parseAccel('Ctrl+' + 'a'.repeat(60), true)).toBeNull()
    expect(parseAccel(42 as unknown as string, true)).toBeNull()
  })

  it('accepts punctuation as a key', () => {
    expect(parseAccel('CmdOrCtrl+,', true)?.key).toBe(',')
    expect(parseAccel('Ctrl+Shift+=', true)).toMatchObject({ shift: true, key: '=' })
  })
})

describe('accelKey', () => {
  it('gives the same form whatever the order or alias', () => {
    expect(accelKey('Shift+CmdOrCtrl+T', true)).toBe('shift+cmd+T')
    expect(accelKey('Cmd+Shift+t', true)).toBe('shift+cmd+T')
    expect(accelKey('CmdOrCtrl+T', false)).toBe('ctrl+T')
  })

  it('treats Shift+= as Plus', () => {
    expect(accelKey('Ctrl+Shift+=', true)).toBe(accelKey('Ctrl+Plus', true))
    expect(accelKey('Ctrl+Shift+Plus', true)).toBe(accelKey('Ctrl+Plus', true))
  })

  it('returns null for an invalid accelerator', () => {
    expect(accelKey('???', true)).toBeNull()
  })
})

describe('usableAccel', () => {
  it('requires ⌘, ⌃ or ⌥ (Shift alone is not enough)', () => {
    expect(usableAccel('Shift+T', true)).toBe(false)
    expect(usableAccel('T', true)).toBe(false)
    expect(usableAccel('Alt+T', true)).toBe(true)
    expect(usableAccel('CmdOrCtrl+T', false)).toBe(true)
  })

  it('allows F1 to F24 keys alone', () => {
    expect(usableAccel('F11', false)).toBe(true)
    expect(usableAccel('Shift+F5', true)).toBe(true)
  })

  it('rejects what cannot be read', () => {
    expect(usableAccel('Ctrl+', true)).toBe(false)
  })
})

describe('accelLabel', () => {
  it('writes with symbols on Mac', () => {
    expect(accelLabel('Ctrl+Alt+Shift+Cmd+T', true)).toBe('⌃⌥⇧⌘T')
    expect(accelLabel('CmdOrCtrl+Left', true)).toBe('⌘←')
    expect(accelLabel('CmdOrCtrl+Backspace', true)).toBe('⌘⌫')
  })

  it('writes in full words elsewhere', () => {
    expect(accelLabel('Shift+CmdOrCtrl+T', false)).toBe('Ctrl+Shift+T')
    expect(accelLabel('Alt+Return', false)).toBe('Alt+Enter')
    expect(accelLabel('Ctrl+Escape', false)).toBe('Ctrl+Esc')
    expect(accelLabel('Cmd+T', false)).toBe('Win+T')
  })

  it('returns the text as is if it is invalid', () => {
    expect(accelLabel('n’importe quoi', true)).toBe('n’importe quoi')
  })
})

describe('raccourcis actifs', () => {
  it("takes the system's default value", () => {
    const lib = shortcutDef('library')!
    expect(defaultAccel(lib, true)).toBe('Cmd+Y')
    expect(defaultAccel(lib, false)).toBe('Ctrl+H')
    expect(defaultAccel(shortcutDef('newTab')!, true)).toBe('CmdOrCtrl+T')
  })

  it('returns an empty array for an unknown command or one without a shortcut', () => {
    expect(shortcutAccels('nope', undefined, true)).toEqual([])
    expect(shortcutAccels('newEasel', undefined, true)).toEqual([])
    expect(shortcutAccel('newEasel', undefined, true)).toBeNull()
  })

  it('joins the default extra shortcuts, the Mac-only ones on Mac only', () => {
    const withAlt = SHORTCUTS.find((s) => s.alt?.length)
    if (withAlt) {
      expect(shortcutAccels(withAlt.id, undefined, false).length).toBeGreaterThan(1)
    }
    const withAltMac = SHORTCUTS.find((s) => s.altMac?.length)
    if (withAltMac) {
      const mac = shortcutAccels(withAltMac.id, undefined, true)
      const other = shortcutAccels(withAltMac.id, undefined, false)
      expect(mac.length).toBeGreaterThan(other.length)
    }
  })

  it('the override replaces everything, extras included; null = none', () => {
    expect(shortcutAccels('newTab', { newTab: 'Alt+X' }, true)).toEqual(['Alt+X'])
    expect(shortcutAccels('newTab', { newTab: null }, true)).toEqual([])
    expect(shortcutAccel('newTab', { newTab: null }, true)).toBeNull()
  })

  it('shortcutLabel gives the text or undefined', () => {
    expect(shortcutLabel('newTab', undefined, true)).toBe('⌘T')
    expect(shortcutLabel('newTab', undefined, false)).toBe('Ctrl+T')
    expect(shortcutLabel('newTab', { newTab: null }, true)).toBeUndefined()
    expect(shortcutLabel('nope', undefined, true)).toBeUndefined()
  })
})

describe('reservedShortcuts', () => {
  it('adds Hide / Minimize on Mac only', () => {
    const labels = (mac: boolean) => reservedShortcuts(mac, false).map((r) => r.label)
    expect(labels(true)).toContain('settings.shortcuts.reserved.hide')
    expect(labels(false)).not.toContain('settings.shortcuts.reserved.hide')
  })

  it('chooses Reset and Full Screen according to the system', () => {
    expect(reservedShortcuts(false, true).find((r) => r.label.endsWith('redo'))?.accel).toBe('Ctrl+Y')
    expect(reservedShortcuts(false, false).find((r) => r.label.endsWith('redo'))?.accel).toBe('Shift+CmdOrCtrl+Z')
    expect(reservedShortcuts(true, false).find((r) => r.label.endsWith('fullScreen'))?.accel).toBe('Ctrl+Cmd+F')
    expect(reservedShortcuts(false, false).find((r) => r.label.endsWith('fullScreen'))?.accel).toBe('F11')
  })

  it('reserves ⌘1…9 and spaces 1…9', () => {
    const list = reservedShortcuts(true, false)
    expect(list.filter((r) => r.label.endsWith('goToTab'))).toHaveLength(9)
    expect(list.filter((r) => r.label.endsWith('goToSpace'))).toHaveLength(9)
    expect(reservedShortcuts(false, false).find((r) => r.accel === 'Alt+3')?.label).toMatch(/goToSpace$/)
  })
})

describe('shortcutConflict', () => {
  it('flags a reserved shortcut', () => {
    expect(shortcutConflict('CmdOrCtrl+C', 'newEasel', undefined, true)).toEqual({ reserved: 'settings.shortcuts.reserved.copy' })
  })

  it('flags another command, but not itself', () => {
    expect(shortcutConflict('CmdOrCtrl+T', 'closeTab', undefined, true)).toEqual({ id: 'newTab' })
    expect(shortcutConflict('CmdOrCtrl+T', 'newTab', undefined, true)).toBeNull()
  })

  it('takes overrides into account', () => {
    expect(shortcutConflict('CmdOrCtrl+T', 'closeTab', { newTab: null }, true)).toBeNull()
    expect(shortcutConflict('Alt+CmdOrCtrl+9', 'closeTab', { newEasel: 'Alt+CmdOrCtrl+9' }, true)).toEqual({ id: 'newEasel' })
  })

  it('compares equivalent forms', () => {
    expect(shortcutConflict('cmd+t', 'closeTab', undefined, true)).toEqual({ id: 'newTab' })
  })

  it('ignores an invalid accelerator', () => {
    expect(shortcutConflict('nope', 'newTab', undefined, true)).toBeNull()
  })
})

describe('takeOver', () => {
  it('gives the shortcut and removes it from the other command', () => {
    expect(takeOver({}, 'newEasel', 'CmdOrCtrl+T', 'newTab', true)).toEqual({ newEasel: 'CmdOrCtrl+T', newTab: null })
  })

  it("keeps the other's main one when it only held the shortcut as an extra", () => {
    const def = SHORTCUTS.find((s) => s.alt?.length)
    if (!def) return
    const extra = def.alt![0]
    const main = defaultAccel(def, false)!
    const out = takeOver({}, 'newEasel', extra, def.id, false)
    expect(out[def.id]).toBe(main)
    expect(out.newEasel).toBe(extra)
  })
})

describe('cleanOverrides', () => {
  it('rejects what is not an object', () => {
    for (const v of [null, undefined, 3, 'x', []]) expect(cleanOverrides(v, true)).toBeNull()
  })

  it('ignores unknown commands', () => {
    expect(cleanOverrides({ nope: 'CmdOrCtrl+K' }, true)).toEqual({})
  })

  it('keeps a valid override', () => {
    expect(cleanOverrides({ newEasel: 'Alt+CmdOrCtrl+9' }, true)).toEqual({ newEasel: 'Alt+CmdOrCtrl+9' })
  })

  it('removes an override equal to the default', () => {
    expect(cleanOverrides({ newTab: 'cmd+t' }, true)).toEqual({})
  })

  it('removes null when there is already no shortcut, keeps it otherwise', () => {
    expect(cleanOverrides({ newEasel: null }, true)).toEqual({})
    expect(cleanOverrides({ newTab: null }, true)).toEqual({ newTab: null })
  })

  it('rejects the whole set if a shortcut is unusable or mistyped', () => {
    expect(cleanOverrides({ newEasel: 'Shift+T' }, true)).toBeNull()
    expect(cleanOverrides({ newEasel: 42 }, true)).toBeNull()
    expect(cleanOverrides({ newEasel: 'Ctrl+' }, true)).toBeNull()
  })

  it('rejects a reserved shortcut', () => {
    expect(cleanOverrides({ newEasel: 'CmdOrCtrl+C' }, true)).toBeNull()
  })

  it('rejects two commands on the same shortcut', () => {
    expect(cleanOverrides({ newEasel: 'Alt+CmdOrCtrl+9', savePage: 'Alt+CmdOrCtrl+9' }, true)).toBeNull()
    expect(cleanOverrides({ newEasel: 'CmdOrCtrl+T' }, true)).toBeNull()
  })

  it('accepts swapping two shortcuts', () => {
    expect(cleanOverrides({ newTab: 'CmdOrCtrl+W', closeTab: 'CmdOrCtrl+T' }, true)).toEqual({
      newTab: 'CmdOrCtrl+W',
      closeTab: 'CmdOrCtrl+T'
    })
  })
})

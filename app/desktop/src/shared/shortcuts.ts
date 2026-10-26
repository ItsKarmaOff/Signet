// Keyboard shortcuts: single list of commands, shared by the menu (main), Settings and the command bar.
// Accelerators written the Electron way (`CmdOrCtrl+Shift+T`). Settings (`settings.shortcuts`) only keep
// the overrides: a string = chosen shortcut, `null` = no shortcut, missing key = default shortcut.

/** True on Windows (main process: `process.platform`; UI: the browser). */
const IS_WIN = (() => {
  const p = (globalThis as { process?: { platform?: string } }).process?.platform
  if (p) return p === 'win32'
  return typeof navigator !== 'undefined' && /Win/i.test(navigator.platform)
})()

export type ShortcutGroup = 'tabs' | 'navigation' | 'spaces' | 'sidebar' | 'split' | 'search' | 'page' | 'windows'

export interface ShortcutDef {
  id: string
  group: ShortcutGroup
  /** Translation key of the label (`ui` namespace). */
  label: string
  /** Default shortcut (null: none, to be chosen). `mac` / `other`: when it differs by system. */
  accel: string | null
  mac?: string
  other?: string
  /** Default extra shortcuts (hidden menu entries), lost as soon as another shortcut is chosen. */
  alt?: string[]
  /** Extra shortcuts on Mac only. */
  altMac?: string[]
}

/** Editable commands, in Settings order. */
export const SHORTCUTS: readonly ShortcutDef[] = [
  { id: 'newTab', group: 'tabs', label: 'settings.shortcuts.item.newTab', accel: 'CmdOrCtrl+T' },
  { id: 'newNote', group: 'tabs', label: 'settings.shortcuts.item.newNote', accel: null, mac: 'Ctrl+Cmd+N', other: 'Ctrl+Alt+Shift+N' },
  { id: 'newEasel', group: 'tabs', label: 'settings.shortcuts.item.newEasel', accel: null },
  { id: 'reopenTab', group: 'tabs', label: 'settings.shortcuts.item.reopenTab', accel: 'Shift+CmdOrCtrl+T' },
  { id: 'closeTab', group: 'tabs', label: 'settings.shortcuts.item.closeTab', accel: 'CmdOrCtrl+W' },
  { id: 'closeWindow', group: 'tabs', label: 'settings.shortcuts.item.closeWindow', accel: 'Shift+CmdOrCtrl+W' },
  { id: 'bookmark', group: 'tabs', label: 'settings.shortcuts.item.bookmark', accel: 'CmdOrCtrl+D' },
  { id: 'pinTab', group: 'tabs', label: 'settings.shortcuts.item.pinTab', accel: 'Alt+CmdOrCtrl+P' },
  { id: 'nextTab', group: 'tabs', label: 'settings.shortcuts.item.nextTab', accel: 'Ctrl+Tab', alt: ['Shift+CmdOrCtrl+]'] },
  { id: 'prevTab', group: 'tabs', label: 'settings.shortcuts.item.prevTab', accel: 'Ctrl+Shift+Tab', alt: ['Shift+CmdOrCtrl+['] },

  { id: 'openAddress', group: 'navigation', label: 'settings.shortcuts.item.openAddress', accel: 'CmdOrCtrl+L' },
  { id: 'back', group: 'navigation', label: 'settings.shortcuts.item.back', accel: 'CmdOrCtrl+[' },
  { id: 'forward', group: 'navigation', label: 'settings.shortcuts.item.forward', accel: 'CmdOrCtrl+]' },
  { id: 'reload', group: 'navigation', label: 'settings.shortcuts.item.reload', accel: 'CmdOrCtrl+R' },
  { id: 'forceReload', group: 'navigation', label: 'settings.shortcuts.item.forceReload', accel: 'Shift+CmdOrCtrl+R' },
  { id: 'copyUrl', group: 'navigation', label: 'settings.shortcuts.item.copyUrl', accel: 'Shift+CmdOrCtrl+C' },
  { id: 'zoomIn', group: 'navigation', label: 'settings.shortcuts.item.zoomIn', accel: 'CmdOrCtrl+Plus', alt: ['CmdOrCtrl+='] },
  { id: 'zoomOut', group: 'navigation', label: 'settings.shortcuts.item.zoomOut', accel: 'CmdOrCtrl+-' },
  { id: 'actualSize', group: 'navigation', label: 'settings.shortcuts.item.actualSize', accel: 'CmdOrCtrl+0' },

  { id: 'nextSpace', group: 'spaces', label: 'settings.shortcuts.item.nextSpace', accel: 'Alt+CmdOrCtrl+Right' },
  { id: 'prevSpace', group: 'spaces', label: 'settings.shortcuts.item.prevSpace', accel: 'Alt+CmdOrCtrl+Left' },

  { id: 'toggleSidebar', group: 'sidebar', label: 'settings.shortcuts.item.toggleSidebar', accel: 'CmdOrCtrl+S' },
  { id: 'compactMode', group: 'sidebar', label: 'settings.shortcuts.item.compactMode', accel: 'Alt+CmdOrCtrl+S' },
  // ⌘⇧S as well on Mac; elsewhere, Ctrl+Shift+S is already the main shortcut.
  { id: 'tabLayout', group: 'sidebar', label: 'settings.shortcuts.item.tabLayout', accel: 'Ctrl+Shift+S', altMac: ['Cmd+Shift+S'] },
  { id: 'bookmarksBar', group: 'sidebar', label: 'settings.shortcuts.item.bookmarksBar', accel: 'Shift+CmdOrCtrl+B' },

  // ⌃⇧= on Mac; elsewhere Ctrl+Shift+= is zoom in (Ctrl+Plus), hence Alt+Shift+=.
  { id: 'openSplit', group: 'split', label: 'settings.shortcuts.item.openSplit', accel: null, mac: 'Ctrl+Shift+=', other: 'Alt+Shift+=' },

  { id: 'find', group: 'search', label: 'settings.shortcuts.item.find', accel: 'CmdOrCtrl+F' },
  { id: 'findNext', group: 'search', label: 'settings.shortcuts.item.findNext', accel: 'CmdOrCtrl+G' },
  { id: 'findPrev', group: 'search', label: 'settings.shortcuts.item.findPrev', accel: 'Shift+CmdOrCtrl+G' },

  { id: 'print', group: 'page', label: 'settings.shortcuts.item.print', accel: 'CmdOrCtrl+P' },
  { id: 'capture', group: 'page', label: 'settings.shortcuts.item.capture', accel: 'Shift+CmdOrCtrl+2' },
  { id: 'reader', group: 'page', label: 'settings.shortcuts.item.reader', accel: 'Alt+CmdOrCtrl+R' },
  { id: 'translate', group: 'page', label: 'settings.shortcuts.item.translate', accel: null },
  { id: 'savePage', group: 'page', label: 'settings.shortcuts.item.savePage', accel: null },
  { id: 'exportPdf', group: 'page', label: 'settings.shortcuts.item.exportPdf', accel: null },
  { id: 'viewSource', group: 'page', label: 'settings.shortcuts.item.viewSource', accel: 'Alt+CmdOrCtrl+U' },

  { id: 'newLittle', group: 'windows', label: 'settings.shortcuts.item.newLittle', accel: 'Alt+CmdOrCtrl+N' },
  { id: 'newPrivate', group: 'windows', label: 'settings.shortcuts.item.newPrivate', accel: 'Shift+CmdOrCtrl+N' },
  { id: 'installApp', group: 'windows', label: 'settings.shortcuts.item.installApp', accel: null },
  { id: 'customizeSite', group: 'windows', label: 'settings.shortcuts.item.customizeSite', accel: 'Alt+CmdOrCtrl+B' },
  { id: 'devTools', group: 'windows', label: 'settings.shortcuts.item.devTools', accel: 'Alt+CmdOrCtrl+I' },
  { id: 'jsConsole', group: 'windows', label: 'settings.shortcuts.item.jsConsole', accel: 'Alt+CmdOrCtrl+J' },
  { id: 'responsive', group: 'windows', label: 'settings.shortcuts.item.responsive', accel: 'Shift+CmdOrCtrl+M' },
  { id: 'library', group: 'windows', label: 'settings.shortcuts.item.library', accel: null, mac: 'Cmd+Y', other: 'Ctrl+H' }
]

export const SHORTCUT_GROUPS: readonly ShortcutGroup[] = ['tabs', 'navigation', 'spaces', 'sidebar', 'split', 'search', 'page', 'windows']

/** Non-editable shortcuts, shown read-only in their group (Mac-style text; `other` elsewhere). */
export const FIXED_SHORTCUTS: readonly { group: ShortcutGroup; label: string; keys: string; other?: string }[] = [
  { group: 'tabs', label: 'settings.shortcuts.item.goToTab', keys: '⌘1 … ⌘9' },
  { group: 'spaces', label: 'settings.shortcuts.item.goToSpace', keys: '⌃1 … ⌃9', other: 'Alt+1 … Alt+9' },
  { group: 'windows', label: 'settings.shortcuts.item.settings', keys: '⌘,' },
  { group: 'windows', label: 'settings.shortcuts.item.fullScreen', keys: '⌃⌘F', other: 'F11' },
  { group: 'windows', label: 'settings.shortcuts.item.quit', keys: '⌘Q' }
]

/**
 * Reserved shortcuts (Edit menu, Quit, Settings, ⌘1…9, ⌃1…9, system): refused for a command, with the name of their
 * owner. Electron accelerators, per system.
 */
export function reservedShortcuts(mac: boolean, win: boolean = IS_WIN): { accel: string; label: string }[] {
  const list: { accel: string; label: string }[] = [
    { accel: 'CmdOrCtrl+Q', label: 'settings.shortcuts.item.quit' },
    { accel: 'CmdOrCtrl+,', label: 'settings.shortcuts.item.settings' },
    { accel: 'CmdOrCtrl+Z', label: 'settings.shortcuts.reserved.undo' },
    { accel: win ? 'Ctrl+Y' : 'Shift+CmdOrCtrl+Z', label: 'settings.shortcuts.reserved.redo' },
    { accel: 'CmdOrCtrl+X', label: 'settings.shortcuts.reserved.cut' },
    { accel: 'CmdOrCtrl+C', label: 'settings.shortcuts.reserved.copy' },
    { accel: 'CmdOrCtrl+V', label: 'settings.shortcuts.reserved.paste' },
    { accel: 'Alt+Shift+CmdOrCtrl+V', label: 'settings.shortcuts.reserved.pasteMatchStyle' },
    { accel: 'CmdOrCtrl+A', label: 'settings.shortcuts.reserved.selectAll' },
    { accel: mac ? 'Ctrl+Cmd+F' : 'F11', label: 'settings.shortcuts.item.fullScreen' }
  ]
  if (mac) {
    list.push(
      { accel: 'Cmd+H', label: 'settings.shortcuts.reserved.hide' },
      { accel: 'Alt+Cmd+H', label: 'settings.shortcuts.reserved.hideOthers' },
      { accel: 'Cmd+M', label: 'settings.shortcuts.reserved.minimize' }
    )
  }
  for (let i = 1; i <= 9; i++) {
    list.push({ accel: `CmdOrCtrl+${i}`, label: 'settings.shortcuts.item.goToTab' })
    list.push({ accel: mac ? `Ctrl+${i}` : `Alt+${i}`, label: 'settings.shortcuts.item.goToSpace' })
  }
  return list
}

export type ShortcutOverrides = Record<string, string | null>

const BY_ID = new Map(SHORTCUTS.map((s) => [s.id, s]))

export const shortcutDef = (id: string): ShortcutDef | undefined => BY_ID.get(id)

/** Default shortcut of a command on this system. */
export function defaultAccel(def: ShortcutDef, mac: boolean): string | null {
  return (mac ? def.mac : def.other) ?? def.accel
}

/** Active shortcuts of a command: the main one first, then the default extras. Empty if none. */
export function shortcutAccels(id: string, overrides: ShortcutOverrides | undefined, mac: boolean): string[] {
  const def = BY_ID.get(id)
  if (!def) return []
  if (overrides && Object.hasOwn(overrides, id)) {
    const custom = overrides[id]
    return custom ? [custom] : []
  }
  const main = defaultAccel(def, mac)
  return main ? [main, ...(def.alt ?? []), ...(mac ? (def.altMac ?? []) : [])] : []
}

/** Main shortcut of a command (for the menu and the command bar), or null. */
export function shortcutAccel(id: string, overrides: ShortcutOverrides | undefined, mac: boolean): string | null {
  return shortcutAccels(id, overrides, mac)[0] ?? null
}

// ───────────────────────── Reading and writing accelerators ─────────────────────────

const MODIFIER_ALIASES: Record<string, 'cmd' | 'ctrl' | 'alt' | 'shift' | 'cmdorctrl'> = {
  command: 'cmd',
  cmd: 'cmd',
  super: 'cmd',
  meta: 'cmd',
  control: 'ctrl',
  ctrl: 'ctrl',
  alt: 'alt',
  option: 'alt',
  shift: 'shift',
  commandorcontrol: 'cmdorctrl',
  cmdorctrl: 'cmdorctrl'
}

const NAMED_KEYS = new Set([
  'Plus',
  'Space',
  'Tab',
  'Backspace',
  'Delete',
  'Insert',
  'Return',
  'Enter',
  'Up',
  'Down',
  'Left',
  'Right',
  'Home',
  'End',
  'PageUp',
  'PageDown',
  'Escape',
  'Esc'
])
const PUNCTUATION = new Set([..."-=[]\\;',./`"])

/** Valid key for Electron, written canonically (`a` → `A`), or null. */
function canonicalKey(key: string): string | null {
  if (/^[a-z0-9]$/i.test(key)) return key.toUpperCase()
  if (/^F([1-9]|1[0-9]|2[0-4])$/i.test(key)) return key.toUpperCase()
  if (PUNCTUATION.has(key)) return key
  const named = [...NAMED_KEYS].find((k) => k.toLowerCase() === key.toLowerCase())
  if (named) return named === 'Enter' ? 'Return' : named === 'Esc' ? 'Escape' : named
  return null
}

export interface ParsedAccel {
  cmd: boolean
  ctrl: boolean
  alt: boolean
  shift: boolean
  key: string
}

/** Reads an Electron accelerator (CmdOrCtrl resolved for the system); null if invalid. */
export function parseAccel(accel: string, mac: boolean): ParsedAccel | null {
  if (typeof accel !== 'string' || accel.length > 60) return null
  // "Plus" is written in full: "+" is only a separator.
  const parts = accel.split('+')
  if (parts.some((p) => !p)) return null
  const key = canonicalKey(parts.pop()!)
  if (!key) return null
  const out: ParsedAccel = { cmd: false, ctrl: false, alt: false, shift: false, key }
  for (const p of parts) {
    const m = MODIFIER_ALIASES[p.toLowerCase()]
    if (!m) return null
    if (m === 'cmdorctrl') out[mac ? 'cmd' : 'ctrl'] = true
    else out[m] = true
  }
  return out
}

/** Comparable form of an accelerator on this system (`cmd+shift+T`), or null if invalid. */
export function accelKey(accel: string, mac: boolean): string | null {
  const p = parseAccel(accel, mac)
  if (!p) return null
  // Disposition US (celle d'Electron) : Plus = Maj+=.
  let { shift, key } = p
  if (key === '=' && shift) key = 'Plus'
  if (key === 'Plus') shift = false
  return [p.ctrl && 'ctrl', p.alt && 'alt', shift && 'shift', p.cmd && 'cmd', key].filter(Boolean).join('+')
}

/** True if the accelerator can serve as a shortcut: at least ⌘, ⌃ or ⌥ (Shift alone is not enough), except F1…F24 keys. */
export function usableAccel(accel: string, mac: boolean): boolean {
  const p = parseAccel(accel, mac)
  if (!p) return false
  return p.cmd || p.ctrl || p.alt || /^F\d+$/.test(p.key)
}

const MAC_KEY_GLYPHS: Record<string, string> = {
  Plus: '+',
  Space: '␣',
  Tab: '⇥',
  Backspace: '⌫',
  Delete: '⌦',
  Return: '↩',
  Up: '↑',
  Down: '↓',
  Left: '←',
  Right: '→',
  Home: '↖',
  End: '↘',
  PageUp: '⇞',
  PageDown: '⇟',
  Escape: '⎋'
}
const OTHER_KEY_NAMES: Record<string, string> = { Plus: '+', Up: '↑', Down: '↓', Left: '←', Right: '→', Return: 'Enter', Escape: 'Esc' }

/** Text of an accelerator: ⌃⌥⇧⌘T on Mac, Ctrl+Alt+Shift+T elsewhere. */
export function accelLabel(accel: string, mac: boolean): string {
  const p = parseAccel(accel, mac)
  if (!p) return accel
  if (mac) return `${p.ctrl ? '⌃' : ''}${p.alt ? '⌥' : ''}${p.shift ? '⇧' : ''}${p.cmd ? '⌘' : ''}${MAC_KEY_GLYPHS[p.key] ?? p.key}`
  return [p.ctrl && 'Ctrl', p.alt && 'Alt', p.shift && 'Shift', p.cmd && 'Win', OTHER_KEY_NAMES[p.key] ?? p.key].filter(Boolean).join('+')
}

/** Text of a command's current shortcut (command bar, tooltips), or undefined if it has none. */
export function shortcutLabel(id: string, overrides: ShortcutOverrides | undefined, mac: boolean): string | undefined {
  const accel = shortcutAccel(id, overrides, mac)
  return accel ? accelLabel(accel, mac) : undefined
}

/**
 * Who already uses this shortcut? Another command (`{ id }`), a reserved shortcut (`{ reserved: label }`), or null.
 * `except`: the command being edited.
 */
export function shortcutConflict(
  accel: string,
  except: string,
  overrides: ShortcutOverrides | undefined,
  mac: boolean
): { id: string } | { reserved: string } | null {
  const key = accelKey(accel, mac)
  if (!key) return null
  for (const r of reservedShortcuts(mac)) if (accelKey(r.accel, mac) === key) return { reserved: r.label }
  for (const def of SHORTCUTS) {
    if (def.id === except) continue
    if (shortcutAccels(def.id, overrides, mac).some((a) => accelKey(a, mac) === key)) return { id: def.id }
  }
  return null
}

const hasSecondary = (def: ShortcutDef, mac: boolean): boolean => !!def.alt?.length || (mac && !!def.altMac?.length)

/**
 * Overrides after giving `accel` to `id` and removing that shortcut from `other`. If `other` only held it as an
 * extra shortcut, it keeps its main one (override equal to the default = "main only").
 */
export function takeOver(overrides: ShortcutOverrides, id: string, accel: string, other: string, mac: boolean): ShortcutOverrides {
  const key = accelKey(accel, mac)
  const accels = shortcutAccels(other, overrides, mac)
  const main = accels[0]
  const onlySecondary = main !== undefined && accelKey(main, mac) !== key
  return { ...overrides, [id]: accel, [other]: onlySecondary ? main : null }
}

/**
 * Cleans overrides coming from the UI: known commands, valid and usable shortcuts, no conflict among themselves
 * or with a reserved shortcut; an override equal to the default shortcut is removed. Null if the set is inconsistent.
 */
export function cleanOverrides(value: unknown, mac: boolean): ShortcutOverrides | null {
  if (!value || typeof value !== 'object' || Array.isArray(value)) return null
  const out: ShortcutOverrides = {}
  for (const [id, accel] of Object.entries(value as Record<string, unknown>)) {
    const def = BY_ID.get(id)
    if (!def) continue
    if (accel === null) {
      if (defaultAccel(def, mac) !== null) out[id] = null
      continue
    }
    if (typeof accel !== 'string' || !usableAccel(accel, mac)) return null
    const main = defaultAccel(def, mac)
    // Equal to the default: removed, unless it has extra shortcuts ("main only", see takeOver).
    if (main && accelKey(main, mac) === accelKey(accel, mac) && !hasSecondary(def, mac)) continue
    out[id] = accel
  }
  // No active shortcut may be used twice.
  const seen = new Map<string, string>()
  const reserved = new Set(reservedShortcuts(mac).map((r) => accelKey(r.accel, mac)))
  for (const def of SHORTCUTS) {
    for (const a of shortcutAccels(def.id, out, mac)) {
      const key = accelKey(a, mac)!
      if (reserved.has(key) && Object.hasOwn(out, def.id)) return null
      const other = seen.get(key)
      // Two default shortcuts may overlap: only overrides count.
      if (other && other !== def.id && (Object.hasOwn(out, def.id) || Object.hasOwn(out, other))) return null
      seen.set(key, def.id)
    }
  }
  return out
}

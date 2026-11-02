// chrome.commands: reading an extension's manifest `commands`, converting Chrome's shortcut syntax to Electron
// accelerators, resolving clashes and localizing descriptions. Pure code (the platform is a parameter), used by main.

export type CommandPlatform = 'darwin' | 'win32' | 'linux'

/** Most suggested shortcuts Chrome honors per extension; the others are ignored. */
export const MAX_SUGGESTED_KEYS = 4

/** Commands that click the extension's toolbar button instead of raising `commands.onCommand`. */
export const EXECUTE_COMMANDS: ReadonlySet<string> = new Set(['_execute_action', '_execute_browser_action', '_execute_page_action'])

export interface ParsedKey {
  /** Command key (macOS only). */
  cmd: boolean
  /** Real Control key. */
  ctrl: boolean
  alt: boolean
  shift: boolean
  /** Electron key name (`Y`, `F5`, `,`, `Up`, `MediaPlayPause`...). */
  key: string
  /** Name of the key in Chrome's syntax (for display). */
  chromeKey: string
}

const MEDIA_KEYS: Record<string, string> = {
  MediaNextTrack: 'MediaNextTrack',
  MediaPlayPause: 'MediaPlayPause',
  MediaPrevTrack: 'MediaPreviousTrack',
  MediaStop: 'MediaStop'
}
const NAMED: Record<string, string> = {
  Comma: ',',
  Period: '.',
  Home: 'Home',
  End: 'End',
  PageUp: 'PageUp',
  PageDown: 'PageDown',
  Space: 'Space',
  Insert: 'Insert',
  Delete: 'Delete',
  Up: 'Up',
  Down: 'Down',
  Left: 'Left',
  Right: 'Right',
  UpArrow: 'Up',
  DownArrow: 'Down',
  LeftArrow: 'Left',
  RightArrow: 'Right'
}

/** Electron key for a Chrome key name, or null. */
function electronKey(chrome: string): string | null {
  if (/^[A-Z0-9]$/.test(chrome)) return chrome
  if (/^F([1-9]|1[0-2])$/.test(chrome)) return chrome
  return NAMED[chrome] ?? MEDIA_KEYS[chrome] ?? null
}

/**
 * Reads a Chrome shortcut (`Ctrl+Shift+Y`) for a platform. Null when Chrome would refuse it: unknown token, no
 * Ctrl/Alt/Command/MacCtrl (media keys excepted), a modifier that does not exist on this system (`Command` or `MacCtrl`
 * outside macOS), or `Search` (ChromeOS only).
 */
export function parseSuggestedKey(text: unknown, platform: CommandPlatform): ParsedKey | null {
  if (typeof text !== 'string' || !text || text.length > 60) return null
  const mac = platform === 'darwin'
  const parts = text.split('+')
  const chromeKey = parts.pop()!
  const key = electronKey(chromeKey)
  if (!key) return null
  const out: ParsedKey = { cmd: false, ctrl: false, alt: false, shift: false, key, chromeKey }
  const seen = new Set<string>()
  for (const p of parts) {
    if (seen.has(p)) return null
    seen.add(p)
    if (p === 'Ctrl') out[mac ? 'cmd' : 'ctrl'] = true
    else if (p === 'Alt') out.alt = true
    else if (p === 'Shift') out.shift = true
    else if (p === 'Command' && mac) out.cmd = true
    else if (p === 'MacCtrl' && mac) out.ctrl = true
    else return null
  }
  if (key in MEDIA_KEYS || chromeKey in MEDIA_KEYS) return out
  if (!out.cmd && !out.ctrl && !out.alt) return null
  return out
}

/** The suggested key of a command for this platform: the platform's own entry, otherwise `default`. */
export function suggestedKeyFor(suggested: unknown, platform: CommandPlatform): string | undefined {
  if (typeof suggested === 'string') return suggested
  if (!suggested || typeof suggested !== 'object') return undefined
  const s = suggested as Record<string, unknown>
  const own = platform === 'darwin' ? 'mac' : platform === 'win32' ? 'windows' : 'linux'
  const value = s[own] ?? s.default
  return typeof value === 'string' ? value : undefined
}

/** Electron accelerator (`Cmd+Shift+Y`). */
export function toAccelerator(p: ParsedKey): string {
  return [p.cmd && 'Cmd', p.ctrl && 'Ctrl', p.alt && 'Alt', p.shift && 'Shift', p.key].filter(Boolean).join('+')
}

/** Comparable form, same shape as `accelKey` in shortcuts.ts (so it can be matched with Signet's own shortcuts). */
export function identityOf(p: ParsedKey): string {
  return [p.ctrl && 'ctrl', p.alt && 'alt', p.shift && 'shift', p.cmd && 'cmd', p.key].filter(Boolean).join('+')
}

const ARROWS: Record<string, string> = { Up: '↑', Down: '↓', Left: '←', Right: '→' }

/** Chrome's display format: `⇧⌘Y` on macOS, `Ctrl+Shift+Y` elsewhere. */
export function displayShortcut(p: ParsedKey, platform: CommandPlatform): string {
  if (platform === 'darwin') return `${p.ctrl ? '⌃' : ''}${p.alt ? '⌥' : ''}${p.shift ? '⇧' : ''}${p.cmd ? '⌘' : ''}${ARROWS[p.key] ?? p.chromeKey}`
  return [p.ctrl && 'Ctrl', p.alt && 'Alt', p.shift && 'Shift', p.chromeKey].filter(Boolean).join('+')
}

export interface ManifestCommand {
  name: string
  /** As written in the manifest (maybe `__MSG_key__`). */
  description: string
  /** The shortcut this platform asks for, or null (none, invalid, or beyond the limit of four). */
  key: ParsedKey | null
}

/** Commands of a manifest, in manifest order. Only the first four valid shortcuts are kept. */
export function readCommands(manifest: unknown, platform: CommandPlatform): ManifestCommand[] {
  const commands = (manifest as { commands?: unknown } | null | undefined)?.commands
  if (!commands || typeof commands !== 'object' || Array.isArray(commands)) return []
  const out: ManifestCommand[] = []
  let kept = 0
  for (const [name, raw] of Object.entries(commands as Record<string, unknown>)) {
    if (!name) continue
    const spec = raw && typeof raw === 'object' ? (raw as Record<string, unknown>) : {}
    let key = parseSuggestedKey(suggestedKeyFor(spec.suggested_key, platform), platform)
    if (key && kept >= MAX_SUGGESTED_KEYS) key = null
    if (key) kept++
    out.push({ name, description: typeof spec.description === 'string' ? spec.description : '', key })
  }
  return out
}

// ───────────────────────── Clashes ─────────────────────────

export interface ExtensionCommandSet {
  id: string
  name: string
  commands: ManifestCommand[]
}

export interface ResolvedCommand {
  extensionId: string
  name: string
  description: string
  /** Electron accelerator when the command owns its shortcut, otherwise null. */
  accel: string | null
  /** Display text of the shortcut, empty when none or lost. */
  shortcut: string
}

export interface Clash {
  extensionId: string
  command: string
  accel: string
  /** Who keeps the shortcut: `Signet` or an extension (`<id> <command>`). */
  owner: string
}

/**
 * Decides who owns each shortcut: Signet's own shortcuts first (`signet`: identity key to owner label), then the
 * extension that came first in `sets`. A loser stays listed, without a shortcut.
 */
export function resolveCommands(
  sets: readonly ExtensionCommandSet[],
  signet: ReadonlyMap<string, string>,
  platform: CommandPlatform
): { commands: ResolvedCommand[]; clashes: Clash[] } {
  const taken = new Map<string, string>()
  const commands: ResolvedCommand[] = []
  const clashes: Clash[] = []
  for (const set of sets) {
    for (const c of set.commands) {
      const base = { extensionId: set.id, name: c.name, description: c.description }
      if (!c.key) {
        commands.push({ ...base, accel: null, shortcut: '' })
        continue
      }
      const id = identityOf(c.key)
      const owner = signet.has(id) ? 'Signet' : taken.get(id)
      if (owner) {
        clashes.push({ extensionId: set.id, command: c.name, accel: toAccelerator(c.key), owner })
        commands.push({ ...base, accel: null, shortcut: '' })
        continue
      }
      taken.set(id, `${set.id} ${c.name}`)
      commands.push({ ...base, accel: toAccelerator(c.key), shortcut: displayShortcut(c.key, platform) })
    }
  }
  return { commands, clashes }
}

// ───────────────────────── Localization ─────────────────────────

export interface MessageEntry {
  message?: string
  placeholders?: Record<string, { content?: string }>
}
export type Messages = Record<string, MessageEntry>

/** Locales to try for `app.getLocale()` (`fr-CA`): `fr_CA`, `fr`, then the manifest's `default_locale`. */
export function localeCandidates(appLocale: string, defaultLocale?: string): string[] {
  const full = appLocale.replace('-', '_')
  const list = [full, full.split('_')[0], defaultLocale].filter((x): x is string => !!x)
  return [...new Set(list)]
}

/**
 * Resolves `__MSG_key__` like Chrome (key lookup ignores case, `$NAME$` placeholders and `$$` are expanded). Other text
 * is returned as is. An unknown key gives an empty string, as Chrome does.
 */
export function localizeText(text: string, candidates: readonly string[], load: (locale: string) => Messages | null): string {
  const m = /^__MSG_([A-Za-z0-9_@]+)__$/.exec(text)
  if (!m) return text
  const key = m[1].toLowerCase()
  for (const locale of candidates) {
    const messages = load(locale)
    if (!messages) continue
    const entry = Object.entries(messages).find(([k]) => k.toLowerCase() === key)?.[1]
    if (typeof entry?.message !== 'string') continue
    const placeholders = Object.fromEntries(Object.entries(entry.placeholders ?? {}).map(([k, v]) => [k.toLowerCase(), v?.content ?? '']))
    return entry.message.replace(/\$([A-Za-z0-9_@]+)\$|\$\$/g, (all, name?: string) => (name ? (placeholders[name.toLowerCase()] ?? all) : '$'))
  }
  return ''
}

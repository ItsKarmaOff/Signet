// Import from other browsers: disk access, dialogs, then writing into the state.
import { app, dialog, shell } from 'electron'
import { execFile } from 'node:child_process'
import { readFile } from 'node:fs/promises'
import { existsSync, readdirSync, statSync } from 'node:fs'
import { dirname, join } from 'node:path'
import type { Browser } from '../../browser/browser'
import { applyBookmarks, applySpaces, type ImportCounts } from './apply'
import { parseSpacesFile } from './arc'
import { parseChromeBookmarks, parseSafariBookmarks, parseXmlPlist } from './bookmarks'
import type { ImportedNode } from './types'
import { extensionIds, profileHas, readHistory, readPasswords, type KeyNames } from './chromium'
import { t } from '../../app/i18n'
import { IMPORT_KINDS, type ImportKind } from '@shared/types'

export type ImportSource = 'arc' | 'chrome' | 'chromium' | 'brave' | 'edge' | 'safari'

interface SourceDef {
  id: ImportSource
  /** Name of the source, for lists. */
  name: string
  /** Key of the action label (with `{{name}}`). */
  titleKey: 'import.fromApp' | 'import.bookmarks' | 'import.bookmarksElided'
  /** Systems where this browser exists. */
  on: NodeJS.Platform[]
  /**
   * Possible locations on this system, in order (the first present wins): the sidebar file (Arc),
   * the bookmarks file (Safari) or the profile folder (Chromium browsers).
   */
  paths: () => string[]
  /** Separate Chromium profile, derived from the location found (Arc: its sidebar and its profile sit side by side). */
  profile?: (located: string) => string
  /** Name of the browser in the system keychain, for its passwords (Chromium browsers). */
  keys?: KeyNames
}

const EVERYWHERE: NodeJS.Platform[] = ['darwin', 'linux', 'win32']

const home = (...parts: string[]) => join(app.getPath('home'), ...parts)
/** Default profile of a Chromium browser (`Default` folder). */
const chromium = (mac: string, linux: string[], win: string): string[] => {
  if (process.platform === 'darwin') return [home('Library/Application Support', mac, 'Default')]
  if (process.platform === 'win32') return [join(process.env.LOCALAPPDATA ?? home('AppData/Local'), win, 'User Data/Default')]
  const config = process.env.XDG_CONFIG_HOME || home('.config')
  // System package, then Flatpak or Snap.
  return linux.map((dir) => (dir.startsWith('~/') ? home(dir.slice(2)) : join(config, dir, 'Default')))
}

/** Arc for Windows lives in an MSIX package whose name ends with a publisher identifier. */
function arcWindows(): string[] {
  const packages = join(process.env.LOCALAPPDATA ?? home('AppData/Local'), 'Packages')
  try {
    return readdirSync(packages)
      .filter((d) => d.startsWith('TheBrowserCompany.Arc'))
      .map((d) => join(packages, d, 'LocalCache/Local/Arc/StorableSidebar.json'))
  } catch {
    return []
  }
}

const ALL_SOURCES: SourceDef[] = [
  {
    id: 'arc',
    name: 'Arc',
    titleKey: 'import.fromApp',
    on: ['darwin', 'win32'],
    paths: () => (process.platform === 'win32' ? arcWindows() : [home('Library/Application Support/Arc/StorableSidebar.json')]),
    // History, passwords and extensions: Arc's Chromium profile, next to its sidebar.
    profile: (sidebar) => join(dirname(sidebar), 'User Data/Default'),
    keys: { mac: 'Arc Safe Storage', linux: [] }
  },
  {
    id: 'chrome',
    name: 'Chrome',
    titleKey: 'import.bookmarks',
    on: EVERYWHERE,
    paths: () => chromium('Google/Chrome', ['google-chrome', '~/.var/app/com.google.Chrome/config/google-chrome/Default'], 'Google/Chrome'),
    keys: { mac: 'Chrome Safe Storage', linux: ['chrome'] }
  },
  {
    id: 'chromium',
    name: 'Chromium',
    titleKey: 'import.bookmarks',
    on: EVERYWHERE,
    paths: () =>
      chromium('Chromium', ['chromium', '~/.var/app/org.chromium.Chromium/config/chromium/Default', '~/snap/chromium/common/chromium/Default'], 'Chromium'),
    keys: { mac: 'Chromium Safe Storage', linux: ['chromium'] }
  },
  {
    id: 'brave',
    name: 'Brave',
    titleKey: 'import.bookmarks',
    on: EVERYWHERE,
    paths: () =>
      chromium(
        'BraveSoftware/Brave-Browser',
        ['BraveSoftware/Brave-Browser', '~/.var/app/com.brave.Browser/config/BraveSoftware/Brave-Browser/Default'],
        'BraveSoftware/Brave-Browser'
      ),
    keys: { mac: 'Brave Safe Storage', linux: ['brave'] }
  },
  {
    id: 'edge',
    name: 'Edge',
    titleKey: 'import.bookmarksElided',
    on: EVERYWHERE,
    paths: () => chromium('Microsoft Edge', ['microsoft-edge', '~/.var/app/com.microsoft.Edge/config/microsoft-edge/Default'], 'Microsoft/Edge'),
    keys: { mac: 'Microsoft Edge Safe Storage', linux: ['microsoft-edge', 'Microsoft Edge'] }
  },
  { id: 'safari', name: 'Safari', titleKey: 'import.bookmarks', on: ['darwin'], paths: () => [home('Library/Safari/Bookmarks.plist')] }
]
/** Sources on this system: the others appear neither in settings nor in the command bar. */
export const IMPORT_SOURCES: SourceDef[] = ALL_SOURCES.filter((s) => s.on.includes(process.platform))
const BOOKMARK_NAMES: Partial<Record<ImportSource, string>> = { chrome: 'Chrome', chromium: 'Chromium', brave: 'Brave', edge: 'Edge' }

/** First location present; a permission denial (folder protected by macOS) counts as present. */
function locate(def: SourceDef): string | null {
  for (const path of def.paths()) {
    try {
      statSync(path)
      return path
    } catch (e) {
      const code = (e as NodeJS.ErrnoException).code
      if (code === 'EPERM' || code === 'EACCES') return path
    }
  }
  return null
}

/** Chromium profile of a source, if it exists. */
function profileOf(def: SourceDef, located: string | null): string | null {
  if (!located || !def.keys) return null
  const dir = def.profile ? def.profile(located) : located
  return existsSync(dir) ? dir : null
}

/** Data the source can provide, in display order (`IMPORT_KINDS`). */
export function kindsOf(def: SourceDef): ImportKind[] {
  const located = locate(def)
  if (!located) return []
  const found = new Set<ImportKind>()
  if (def.id === 'arc') found.add('spaces')
  if (def.id === 'safari') found.add('bookmarks')
  const profile = profileOf(def, located)
  if (profile) {
    if (def.id !== 'arc' && existsSync(join(profile, 'Bookmarks'))) found.add('bookmarks')
    const has = profileHas(profile)
    if (has.history) found.add('history')
    if (has.passwords) found.add('passwords')
    if (has.extensions) found.add('extensions')
  }
  return IMPORT_KINDS.filter((k) => found.has(k))
}

/** Sources present on disk, with the data they can provide. */
export function availableSources(): (SourceDef & { kinds: ImportKind[] })[] {
  const now = Date.now()
  if (sourcesCache && now - sourcesCache.at < 10_000) return sourcesCache.list
  const list = IMPORT_SOURCES.map((def) => ({ ...def, kinds: kindsOf(def) })).filter((def) => def.kinds.length > 0)
  sourcesCache = { at: now, list }
  return list
}

// Avoids re-reading the disk on each keystroke in the palette.
let sourcesCache: { at: number; list: (SourceDef & { kinds: ImportKind[] })[] } | null = null

const PERMISSION = /not permitted|permission|EPERM|EACCES/i

function plutil(args: string[]): Promise<string> {
  return new Promise((resolve, reject) => {
    execFile('plutil', args, { maxBuffer: 256 * 1024 * 1024 }, (err, stdout, stderr) => (err ? reject(new Error(`${stderr || err.message}`)) : resolve(stdout)))
  })
}

async function readSafari(path: string): Promise<ImportedNode | null> {
  try {
    return parseSafariBookmarks(JSON.parse(await plutil(['-convert', 'json', '-o', '-', path])))
  } catch (e) {
    if (PERMISSION.test((e as Error).message)) throw e
    // JSON rejects binary data and dates: fall back to XML.
    return parseSafariBookmarks(parseXmlPlist(await plutil(['-convert', 'xml1', '-o', '-', path])))
  }
}

export function summarize(c: ImportCounts, folderName?: string): string {
  if (c.bookmarks)
    return folderName ? t('import.summary.bookmarks', { count: c.bookmarks, folder: folderName }) : t('import.summary.bookmarksBar', { count: c.bookmarks })
  const parts = [
    t('import.summary.spaces', { count: c.spaces }),
    t('import.summary.pinned', { count: c.pinned }),
    t('import.summary.favorites', { count: c.favorites })
  ]
  return t('import.summary.full', { parts: parts.join(', '), imported: t('import.summary.tabs', { count: c.today }) })
}

/** Favorites (Chrome, Brave, Edge, Chromium, Safari) or spaces (Arc): a summary line, and a note if a limit was hit. */
async function importMain(b: Browser, def: SourceDef, path: string, profile: string | null): Promise<{ line: string; note?: string }> {
  let counts: ImportCounts
  let folderName: string | undefined
  if (def.id === 'arc') {
    const data = parseSpacesFile(JSON.parse(await readFile(path, 'utf8')))
    if (data.spaces.length === 0 && data.favorites.length === 0) throw new Error(t('import.error.noContent'))
    counts = applySpaces(b.store.state, data)
  } else {
    const folder =
      def.id === 'safari'
        ? await readSafari(path)
        : parseChromeBookmarks(JSON.parse(await readFile(join(profile ?? path, 'Bookmarks'), 'utf8')), BOOKMARK_NAMES[def.id] ?? def.name)
    if (!folder) throw new Error(t('import.error.noBookmarks'))
    const done = applyBookmarks(b.space, folder)
    folderName = done.inFolder && folder.kind === 'folder' ? folder.name : undefined
    counts = done
    // Imported bookmarks must be visible: the bar is shown.
    if (b.store.state.settings.showBookmarksBar === false) b.store.state.settings.showBookmarksBar = true
  }
  b.store.scheduleBookmarks()
  b.broadcast.changed()
  return {
    line: summarize(counts, folderName),
    note: counts.truncated ? t(counts.bookmarks > 0 ? 'import.truncatedBookmarks' : 'import.truncated') : undefined
  }
}

/** Probable reason for an unreadable password, depending on the system. */
const passwordHint = () =>
  t(
    process.platform === 'darwin' ? 'import.passwordsFailedMac' : process.platform === 'win32' ? 'import.passwordsFailedWindows' : 'import.passwordsFailedLinux'
  )

/**
 * Imports the checked data from a source (all it has if `kinds` is absent), in the order of `IMPORT_KINDS`.
 * Each item is independent: a failure does not stop the others. A summary is shown at the end.
 */
export async function runImport(b: Browser, id: ImportSource, kinds?: ImportKind[]): Promise<void> {
  const def = IMPORT_SOURCES.find((s) => s.id === id)
  if (!def) return
  const show = (options: Electron.MessageBoxOptions) => (b.win ? dialog.showMessageBox(b.win, options) : dialog.showMessageBox(options))
  const path = locate(def)
  const offered = kindsOf(def)
  const wanted = kinds ? offered.filter((k) => kinds.includes(k)) : offered
  if (!path || wanted.length === 0) return
  const profile = profileOf(def, path)
  const lines: string[] = []
  const notes: string[] = []
  const errors: string[] = []
  for (const kind of wanted) {
    try {
      if (kind === 'spaces' || kind === 'bookmarks') {
        const r = await importMain(b, def, path, profile)
        lines.push(r.line)
        if (r.note) notes.push(r.note)
      } else if (!profile) {
        continue
      } else if (kind === 'history') {
        const added = b.history.importEntries(await readHistory(profile))
        lines.push(t('import.summary.history', { count: added }))
      } else if (kind === 'passwords') {
        if (!b.passwords.encryption) throw new Error(t('passwords.error.encryption'))
        const { logins, failed } = await readPasswords(profile, def.keys!)
        const { added, existing } = b.passwords.addMany(logins)
        lines.push(t('import.summary.passwords', { count: added }))
        if (existing > 0) notes.push(t('import.passwordsExisting', { count: existing }))
        if (failed > 0) notes.push(`${t('import.passwordsFailed', { count: failed })} ${passwordHint()}`)
      } else if (kind === 'extensions') {
        const have = new Set(b.extensions.list().map((e) => e.id))
        const ids = extensionIds(profile).filter((x) => !have.has(x))
        lines.push(t('import.summary.extensions', { count: ids.length }))
        // Downloaded from the store, one after the other, without blocking the summary.
        void (async () => {
          for (const x of ids) await b.extensions.installById(x)
        })()
      }
    } catch (e) {
      const message = (e as Error).message ?? String(e)
      if (id === 'safari' && PERMISSION.test(message)) {
        const { response } = await show({
          type: 'warning',
          message: t('import.safariDenied.message'),
          detail: t(app.isPackaged ? 'import.safariDenied.detailPackaged' : 'import.safariDenied.detailDev'),
          buttons: [t('import.safariDenied.openSettings'), t('dialog.cancel')],
          defaultId: 0,
          cancelId: 1
        })
        if (response === 0) void shell.openExternal('x-apple.systempreferences:com.apple.preference.security?Privacy_AllFiles')
        return
      }
      errors.push(t('import.kindFailed', { kind: t(`import.kind.${kind}`), error: message }))
    }
  }
  sourcesCache = null
  const detail = [...lines, ...errors].join('\n') + (notes.length ? `\n\n${notes.join('\n')}` : '')
  if (lines.length === 0) await show({ type: 'error', message: t('import.failed'), detail, buttons: [t('dialog.ok')] })
  else await show({ type: errors.length ? 'warning' : 'info', message: t('import.done'), detail, buttons: [t('dialog.ok')] })
}

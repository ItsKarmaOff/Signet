// Profile of a Chromium browser (Chrome, Chromium, Brave, Edge, Arc): history, extensions and passwords.
// SQLite databases are copied before reading (the open browser locks them). See FORMATS.md.
import { execFile } from 'node:child_process'
import { createDecipheriv, pbkdf2Sync } from 'node:crypto'
import { existsSync, readdirSync, readFileSync } from 'node:fs'
import { copyFile, mkdtemp, rm } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { dirname, join } from 'node:path'
import type { StatementSync } from 'node:sqlite'

/** Name of the browser in the system keychain (encryption key of its passwords). */
export interface KeyNames {
  /** macOS: Keychain service ("Brave Safe Storage"). */
  mac: string
  /** Linux: value of the `application` attribute in the Secret Service keyring (GNOME), in the order tried. */
  linux: string[]
}

/** Maximum number of history pages read (the most recent): Signet's history keeps 5,000. */
const HISTORY_LIMIT = 5000
/** Microseconds between January 1, 1601 (Chromium's clock) and January 1, 1970. */
const EPOCH_DELTA_US = 11_644_473_600_000_000n

const chromeTime = (us: unknown): number => {
  try {
    const v = typeof us === 'bigint' ? us : BigInt(Math.trunc(Number(us) || 0))
    return v > EPOCH_DELTA_US ? Number((v - EPOCH_DELTA_US) / 1000n) : 0
  } catch {
    return 0
  }
}

/** Presence of each data file in the profile. */
export function profileHas(profile: string): { history: boolean; passwords: boolean; extensions: boolean } {
  return {
    history: existsSync(join(profile, 'History')),
    passwords: existsSync(join(profile, 'Login Data')),
    extensions: extensionIds(profile).length > 0
  }
}

/** Copies a database (and its WAL journal if any) into a temporary folder, opens it read-only and runs the query. */
async function query<T>(file: string, sql: string, read: (stmt: StatementSync) => T): Promise<T> {
  const dir = await mkdtemp(join(tmpdir(), 'signet-import-'))
  try {
    const copy = join(dir, 'db')
    await copyFile(file, copy)
    if (existsSync(`${file}-wal`)) await copyFile(`${file}-wal`, `${copy}-wal`)
    // Module built into Node, loaded at runtime (the bundler does not need to know it).
    const { DatabaseSync } = process.getBuiltinModule('node:sqlite')
    const db = new DatabaseSync(copy, { readOnly: true })
    try {
      const stmt = db.prepare(sql)
      // Chromium dates (microseconds since 1601) exceed JavaScript's safe integers.
      stmt.setReadBigInts(true)
      return read(stmt)
    } finally {
      db.close()
    }
  } finally {
    await rm(dir, { recursive: true, force: true }).catch(() => {})
  }
}

/** Visited pages, most recent first (hidden pages excluded). */
export async function readHistory(profile: string): Promise<{ url: string; title: string; visits: number; lastVisit: number }[]> {
  const rows = await query(
    join(profile, 'History'),
    `SELECT url, title, visit_count, last_visit_time FROM urls WHERE hidden = 0 AND url LIKE 'http%' ORDER BY last_visit_time DESC LIMIT ${HISTORY_LIMIT}`,
    (s) => s.all() as { url: string; title: string | null; visit_count: number | bigint; last_visit_time: number | bigint }[]
  )
  return rows
    .map((r) => ({ url: String(r.url), title: String(r.title ?? ''), visits: Number(r.visit_count) || 1, lastVisit: chromeTime(r.last_visit_time) }))
    .filter((r) => /^https?:\/\//i.test(r.url) && r.lastVisit > 0)
}

type Obj = Record<string, unknown>
const isObj = (v: unknown): v is Obj => !!v && typeof v === 'object' && !Array.isArray(v)

/** Extension settings (`extensions.settings`), in `Secure Preferences` (recent) and `Preferences`. */
function extensionSettings(profile: string): Obj {
  const out: Obj = {}
  for (const name of ['Preferences', 'Secure Preferences']) {
    try {
      const json = JSON.parse(readFileSync(join(profile, name), 'utf8')) as unknown
      const settings = isObj(json) && isObj(json.extensions) && isObj(json.extensions.settings) ? json.extensions.settings : {}
      Object.assign(out, settings)
    } catch {
      // file missing or unreadable
    }
  }
  return out
}

/** True if the extension is disabled: `state: 0` (old versions) or a disable reason (`disable_reasons`). */
function disabled(s: Obj): boolean {
  const reasons = s.disable_reasons
  return s.state === 0 || (typeof reasons === 'number' && reasons !== 0) || (Array.isArray(reasons) && reasons.length > 0)
}

/**
 * Extensions installed from the store and enabled: folders of `Extensions/`, except those the browser installs
 * itself (`was_installed_by_default`), that do not come from the store (`from_webstore: false`) or that are disabled
 * (Signet does not keep an extension that is installed but disabled: importing it would enable it).
 */
export function extensionIds(profile: string): string[] {
  let dirs: string[]
  try {
    dirs = readdirSync(join(profile, 'Extensions')).filter((d) => /^[a-p]{32}$/.test(d))
  } catch {
    return []
  }
  const settings = extensionSettings(profile)
  return dirs.filter((id) => {
    const s = settings[id]
    if (!isObj(s)) return true
    return s.was_installed_by_default !== true && s.from_webstore !== false && !disabled(s)
  })
}

function run(cmd: string, args: string[]): Promise<string> {
  return new Promise((resolve, reject) =>
    execFile(cmd, args, { timeout: 120_000, maxBuffer: 1024 * 1024 }, (err, stdout) => (err ? reject(err) : resolve(String(stdout).trim())))
  )
}

const SALT = 'saltysalt'
const CBC_IV = Buffer.alloc(16, 0x20)

function decryptCbc(key: Buffer, blob: Buffer): string {
  const d = createDecipheriv('aes-128-cbc', key, CBC_IV)
  return Buffer.concat([d.update(blob.subarray(3)), d.final()]).toString('utf8')
}

/** Decryption keys depending on the system; null = key not found (the browser was never opened, keychain locked…). */
interface Keys {
  v10: Buffer | null
  v11: Buffer | null
  gcm: Buffer | null
}

async function macKeys(names: KeyNames): Promise<Keys> {
  // macOS asks the user to allow Signet to read this Keychain entry.
  const secret = await run('security', ['find-generic-password', '-w', '-s', names.mac]).catch(() => '')
  return { v10: secret ? pbkdf2Sync(secret, SALT, 1003, 16, 'sha1') : null, v11: null, gcm: null }
}

async function linuxKeys(names: KeyNames): Promise<Keys> {
  // v10: Chromium's fixed password on Linux; v11: GNOME keyring secret (libsecret's `secret-tool` tool).
  let secret = ''
  for (const app of names.linux) {
    secret = await run('secret-tool', ['lookup', 'application', app]).catch(() => '')
    if (secret) break
  }
  return { v10: pbkdf2Sync('peanuts', SALT, 1, 16, 'sha1'), v11: secret ? pbkdf2Sync(secret, SALT, 1, 16, 'sha1') : null, gcm: null }
}

async function windowsKeys(profile: string): Promise<Keys> {
  // AES-256 profile key, protected by DPAPI in `Local State`; PowerShell decrypts it for the current user.
  try {
    const state = JSON.parse(readFileSync(join(dirname(profile), 'Local State'), 'utf8')) as { os_crypt?: { encrypted_key?: string } }
    const wrapped = Buffer.from(state.os_crypt?.encrypted_key ?? '', 'base64')
    if (wrapped.subarray(0, 5).toString() !== 'DPAPI') return { v10: null, v11: null, gcm: null }
    const b64 = wrapped.subarray(5).toString('base64')
    const script =
      'Add-Type -AssemblyName System.Security;' +
      `[Convert]::ToBase64String([Security.Cryptography.ProtectedData]::Unprotect([Convert]::FromBase64String('${b64}'), $null, 'CurrentUser'))`
    const key = Buffer.from(await run('powershell.exe', ['-NoProfile', '-NonInteractive', '-Command', script]), 'base64')
    return { v10: null, v11: null, gcm: key.length === 32 ? key : null }
  } catch {
    return { v10: null, v11: null, gcm: null }
  }
}

function decrypt(blob: Buffer, keys: Keys): string | null {
  if (blob.length === 0) return null
  const tag = blob.subarray(0, 3).toString('latin1')
  try {
    if (process.platform === 'win32') {
      // v10: AES-256-GCM (12-byte nonce, 16-byte tag). v20 ("app-bound", recent Chrome): unreadable outside the browser.
      if (tag !== 'v10' || !keys.gcm) return null
      const d = createDecipheriv('aes-256-gcm', keys.gcm, blob.subarray(3, 15))
      d.setAuthTag(blob.subarray(blob.length - 16))
      return Buffer.concat([d.update(blob.subarray(15, blob.length - 16)), d.final()]).toString('utf8')
    }
    if (tag === 'v10' && keys.v10) return decryptCbc(keys.v10, blob)
    if (tag === 'v11' && keys.v11) return decryptCbc(keys.v11, blob)
    return null
  } catch {
    return null
  }
}

/**
 * Saved credentials, decrypted with the key the browser keeps in the system keychain.
 * `failed`: those that could not be decrypted (key refused or not found, browser-bound encryption).
 */
export async function readPasswords(
  profile: string,
  names: KeyNames
): Promise<{ logins: { origin: string; username: string; password: string; createdAt: number }[]; failed: number }> {
  const rows = await query(
    join(profile, 'Login Data'),
    // Most recent first: for the same site and username saved twice, that one is kept.
    'SELECT origin_url, signon_realm, username_value, password_value, date_created FROM logins WHERE blacklisted_by_user = 0 ORDER BY date_created DESC',
    (s) =>
      s.all() as {
        origin_url: string
        signon_realm: string
        username_value: string | null
        password_value: Uint8Array | null
        date_created: number | bigint
      }[]
  )
  const web = rows.filter((r) => /^https?:\/\//i.test(String(r.signon_realm || r.origin_url)))
  if (web.length === 0) return { logins: [], failed: 0 }
  const keys = process.platform === 'darwin' ? await macKeys(names) : process.platform === 'win32' ? await windowsKeys(profile) : await linuxKeys(names)
  const logins: { origin: string; username: string; password: string; createdAt: number }[] = []
  let failed = 0
  for (const r of web) {
    const password = r.password_value ? decrypt(Buffer.from(r.password_value), keys) : null
    if (!password) {
      failed++
      continue
    }
    logins.push({ origin: String(r.origin_url || r.signon_realm), username: String(r.username_value ?? ''), password, createdAt: chromeTime(r.date_created) })
  }
  return { logins, failed }
}

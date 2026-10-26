// Password manager: pure logic (origins, matching with a page, save / update, CSV import and export).
// Storage (Keychain) and display live in the app; here nothing touches the platform.
import { getDomain } from 'tldts'

export interface Credential {
  id: string
  /** http(s) origin: `https://example.com` (no path). */
  origin: string
  username: string
  password: string
  createdAt: number
  /** Last fill, or `null` if never used. */
  lastUsed: number | null
}

/** Credential without its password: all we keep in memory for lists. */
export type CredentialMeta = Omit<Credential, 'password'>

/** Maximum size of a field (username, password). */
export const MAX_FIELD = 1000

/** http(s) origin of an input (`example.com`, `https://example.com/login`…), or `null`. */
export function normalizeOrigin(input: string): string | null {
  const text = input.trim()
  if (!text || /\s/.test(text)) return null
  try {
    const u = new URL(/^[a-z][a-z0-9+.-]*:\/\//i.test(text) ? text : `https://${text}`)
    return (u.protocol === 'https:' || u.protocol === 'http:') && u.hostname ? u.origin : null
  } catch {
    return null
  }
}

/** Host name of an origin, without `www.`. */
export const hostOf = (origin: string): string => {
  try {
    return new URL(origin).hostname.replace(/^www\./i, '').toLowerCase()
  } catch {
    return origin
  }
}

/**
 * Does a credential saved for `credOrigin` apply to the page `pageUrl`?
 * - same host (give or take `www.`), same https protocol;
 * - or page on a subdomain of a saved domain (`example.com` serves `account.example.com`), never of a public suffix (`github.io`);
 * - an http page only accepts the identical http origin: never an https password poured onto a plain-text page.
 */
export function matchesPage(credOrigin: string, pageUrl: string): boolean {
  let cred: URL
  let page: URL
  try {
    cred = new URL(credOrigin)
    page = new URL(pageUrl)
  } catch {
    return false
  }
  if (page.protocol !== 'https:' && page.protocol !== 'http:') return false
  if (page.origin === cred.origin) return true
  if (page.protocol !== 'https:' || cred.protocol !== 'https:' || cred.port !== page.port) return false
  const c = hostOf(credOrigin)
  const p = page.hostname.replace(/^www\./i, '').toLowerCase()
  if (c === p) return true
  return p.endsWith(`.${c}`) && getDomain(c, { allowPrivateDomains: true }) === c
}

/** Credentials that apply on `pageUrl`, most recently used first. */
export function credentialsForPage<T extends CredentialMeta>(list: readonly T[], pageUrl: string): T[] {
  return list.filter((c) => matchesPage(c.origin, pageUrl)).sort((a, b) => (b.lastUsed ?? b.createdAt) - (a.lastUsed ?? a.createdAt))
}

export interface Submission {
  origin: string
  username: string
  password: string
}

export type SavePlan = { action: 'none' } | { action: 'save' } | { action: 'update'; id: string }

/**
 * What to do with a credential typed in a page? Compares with credentials already saved for the same origin:
 * nothing if an identical one exists, an update if the username exists with another password, otherwise a new save.
 */
export function planSave(existing: readonly Credential[], sub: Submission): SavePlan {
  const username = sub.username.trim()
  if (!sub.password || sub.password.length > MAX_FIELD || username.length > MAX_FIELD) return { action: 'none' }
  const here = existing.filter((c) => c.origin === sub.origin)
  const same = here.find((c) => c.username.toLowerCase() === username.toLowerCase())
  if (same) return same.password === sub.password ? { action: 'none' } : { action: 'update', id: same.id }
  // Input without a username (field not detected): if this password is already there, it is not new.
  if (!username && here.some((c) => c.password === sub.password)) return { action: 'none' }
  return { action: 'save' }
}

/** Short unique identifier for a new entry. */
export const newCredentialId = (now = Date.now()): string => `${now.toString(36)}${Math.random().toString(36).slice(2, 8)}`

// ───────────── CSV ─────────────

/** Reads a CSV (RFC 4180: quotes, doubled quotes, line breaks inside a field). Empty lines ignored. */
export function parseCsv(text: string): string[][] {
  const rows: string[][] = []
  let row: string[] = []
  let field = ''
  let quoted = false
  const src = text.charCodeAt(0) === 0xfeff ? text.slice(1) : text
  for (let i = 0; i < src.length; i++) {
    const c = src[i]
    if (quoted) {
      if (c === '"' && src[i + 1] === '"') {
        field += '"'
        i++
      } else if (c === '"') quoted = false
      else field += c
    } else if (c === '"') quoted = true
    else if (c === ',') {
      row.push(field)
      field = ''
    } else if (c === '\n' || c === '\r') {
      if (c === '\r' && src[i + 1] === '\n') i++
      row.push(field)
      rows.push(row)
      row = []
      field = ''
    } else field += c
  }
  if (field || row.length) {
    row.push(field)
    rows.push(row)
  }
  return rows.filter((r) => r.some((f) => f.trim()))
}

const csvField = (s: string) => (/[",\r\n]/.test(s) ? `"${s.replace(/"/g, '""')}"` : s)

// Columns of the exports from Passwords (Apple), Chrome, Firefox, Bitwarden, 1Password, LastPass, Proton Pass, Dashlane.
const URL_COLUMNS = ['url', 'login_uri', 'website', 'origin', 'login url', 'web site', 'hostname']
const USER_COLUMNS = ['username', 'login_username', 'login', 'user', 'user name', 'email']
const PASSWORD_COLUMNS = ['password', 'login_password']

export interface ImportRow {
  origin: string
  username: string
  password: string
  createdAt?: number
}

/** Importable rows of a CSV, or `null` if the format is not recognized (address and password columns not found). */
export function parsePasswordCsv(text: string): ImportRow[] | null {
  const rows = parseCsv(text)
  const header = (rows.shift() ?? []).map((h) => h.trim().toLowerCase())
  const col = (names: string[]) => names.map((n) => header.indexOf(n)).find((i) => i >= 0) ?? -1
  const ui = col(URL_COLUMNS)
  const pi = col(PASSWORD_COLUMNS)
  if (ui < 0 || pi < 0) return null
  const user = USER_COLUMNS.map((n) => header.indexOf(n)).filter((i) => i >= 0)
  const out: ImportRow[] = []
  for (const row of rows) {
    const origin = normalizeOrigin(row[ui] ?? '')
    const password = row[pi] ?? ''
    // Proton Pass splits username and email: the first non-empty one wins.
    const username = (user.map((i) => (row[i] ?? '').trim()).find(Boolean) ?? '').slice(0, MAX_FIELD)
    if (!origin || !password || password.length > MAX_FIELD) continue
    out.push({ origin, username, password })
  }
  return out
}

/**
 * Splits imported rows: those to add (the first duplicate in the list wins) and the number of those already saved
 * (same origin and same username: kept as they are).
 */
export function planImport(existing: readonly CredentialMeta[], rows: readonly ImportRow[]): { toAdd: ImportRow[]; existing: number } {
  const known = new Set(existing.map((c) => `${c.origin}\n${c.username.toLowerCase()}`))
  const counted = new Set<string>()
  const listed = new Set<string>()
  const toAdd: ImportRow[] = []
  let already = 0
  for (const r of rows) {
    const key = `${r.origin}\n${r.username.toLowerCase()}`
    if (known.has(key)) {
      if (!counted.has(key)) already++
      counted.add(key)
      continue
    }
    if (listed.has(key)) continue
    listed.add(key)
    toAdd.push(r)
  }
  return { toAdd, existing: already }
}

/** Export in Chrome's format (`name,url,username,password,note`), readable by other browsers and managers. */
export function exportPasswordCsv(list: readonly Pick<Credential, 'origin' | 'username' | 'password'>[]): string {
  const lines = ['name,url,username,password,note']
  for (const c of list) lines.push([hostOf(c.origin), c.origin, c.username, c.password, ''].map(csvField).join(','))
  return `${lines.join('\n')}\n`
}

// ───────────── Never save ─────────────

/** Adds an origin to the "never save" list, without duplicates. */
export const addNever = (list: readonly string[], origin: string): string[] => (list.includes(origin) ? [...list] : [...list, origin])

/** Is this page on the "never save" list? */
export const isNever = (list: readonly string[], pageUrl: string): boolean => list.some((o) => matchesPage(o, pageUrl))

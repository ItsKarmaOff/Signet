// Downloads: saved list (name, address, size, date, state, local path) and pure operations on it.
// The download itself (expo-file-system) lives in the mobile app; here, data only.

export type DownloadStatus = 'downloading' | 'done' | 'failed'

export interface DownloadEntry {
  id: string
  name: string
  url: string
  /** Size in bytes (0 while unknown). */
  size: number
  /** Start of the download (ms). */
  date: number
  status: DownloadStatus
  /** `file://` address of the file; empty until the download is finished. */
  path: string
  /** Progress from 0 to 1 while downloading (never saved). */
  progress?: number
}

export const DOWNLOADS_CAP = 500

export type DownloadKind = 'image' | 'video' | 'audio' | 'pdf' | 'archive' | 'document' | 'spreadsheet' | 'presentation' | 'text' | 'code' | 'app' | 'other'

const KINDS: Record<DownloadKind, string[]> = {
  image: ['jpg', 'jpeg', 'png', 'gif', 'webp', 'heic', 'heif', 'svg', 'bmp', 'tif', 'tiff', 'avif', 'ico'],
  video: ['mp4', 'mov', 'm4v', 'mkv', 'avi', 'webm', 'wmv'],
  audio: ['mp3', 'm4a', 'aac', 'wav', 'flac', 'ogg', 'opus', 'aiff'],
  pdf: ['pdf'],
  archive: ['zip', 'rar', '7z', 'tar', 'gz', 'tgz', 'bz2', 'xz'],
  document: ['doc', 'docx', 'odt', 'rtf', 'pages', 'epub'],
  spreadsheet: ['xls', 'xlsx', 'ods', 'csv', 'numbers'],
  presentation: ['ppt', 'pptx', 'odp', 'key'],
  text: ['txt', 'md', 'log'],
  code: ['json', 'xml', 'html', 'htm', 'js', 'ts', 'css', 'yml', 'yaml'],
  app: ['ipa', 'apk', 'dmg', 'exe', 'pkg', 'deb', 'msi'],
  other: []
}

export const fileExtension = (name: string): string => {
  const i = name.lastIndexOf('.')
  return i > 0 && i < name.length - 1 ? name.slice(i + 1).toLowerCase() : ''
}

/** File family based on its extension (used to pick the icon). */
export function downloadKind(name: string): DownloadKind {
  const ext = fileExtension(name)
  if (!ext) return 'other'
  for (const kind of Object.keys(KINDS) as DownloadKind[]) if (KINDS[kind].includes(ext)) return kind
  return 'other'
}

/** Makes a name usable as a file name: no separator or forbidden character, no leading dot, bounded length (extension kept). */
export function sanitizeFileName(name: string, fallback = 'download'): string {
  let clean = [...name].map((ch) => (ch.charCodeAt(0) < 32 || ch.charCodeAt(0) === 127 ? '_' : ch)).join('')
  clean = clean
    .replace(/[/\\:*?"<>|]+/g, '_')
    .replace(/\s+/g, ' ')
    .trim()
  clean = clean.replace(/^\.+/, '').replace(/[. ]+$/, '')
  if (!clean) return fallback
  if (clean.length > 120) {
    const ext = fileExtension(clean)
    const keep = ext && ext.length <= 10 ? `.${ext}` : ''
    clean = clean.slice(0, 120 - keep.length) + keep
  }
  return clean
}

/** Name suggested by the address: last path segment, decoded; `download` if there is none. */
export function fileNameFromUrl(url: string, fallback = 'download'): string {
  try {
    const last = new URL(url).pathname.split('/').filter(Boolean).pop() ?? ''
    let decoded = last
    try {
      decoded = decodeURIComponent(last)
    } catch {
      // malformed percent-encoding: keep the text as is
    }
    return sanitizeFileName(decoded, fallback)
  } catch {
    return fallback
  }
}

/** Adds ` (1)`, ` (2)`… before the extension while the name is already taken (case-insensitive). */
export function uniqueFileName(name: string, taken: readonly string[]): string {
  const used = new Set(taken.map((n) => n.toLowerCase()))
  if (!used.has(name.toLowerCase())) return name
  const ext = fileExtension(name)
  const stem = ext ? name.slice(0, name.length - ext.length - 1) : name
  for (let i = 1; ; i++) {
    const candidate = ext ? `${stem} (${i}).${ext}` : `${stem} (${i})`
    if (!used.has(candidate.toLowerCase())) return candidate
  }
}

/** Readable size: `512 B`, `1,4 MB`… (decimal separator given by the caller, `.` by default). */
export function formatBytes(bytes: number, decimal = '.'): string {
  if (!Number.isFinite(bytes) || bytes <= 0) return ''
  const units = ['B', 'KB', 'MB', 'GB', 'TB']
  let value = bytes
  let i = 0
  while (value >= 1000 && i < units.length - 1) {
    value /= 1000
    i++
  }
  const text = i === 0 || value >= 100 ? String(Math.round(value)) : value.toFixed(1).replace(/\.0$/, '')
  return `${text.replace('.', decimal)} ${units[i]}`
}

export const addDownload = (list: readonly DownloadEntry[], entry: DownloadEntry): DownloadEntry[] =>
  [entry, ...list.filter((d) => d.id !== entry.id)].slice(0, DOWNLOADS_CAP)

export function updateDownload(list: readonly DownloadEntry[], id: string, patch: Partial<Omit<DownloadEntry, 'id'>>): DownloadEntry[] {
  if (!list.some((d) => d.id === id)) return list as DownloadEntry[]
  return list.map((d) => (d.id === id ? { ...d, ...patch } : d))
}

export const removeDownload = (list: readonly DownloadEntry[], id: string): DownloadEntry[] => list.filter((d) => d.id !== id)

/** Clears the list, except downloads in progress (we do not lose track of them). */
export const clearDownloads = (list: readonly DownloadEntry[]): DownloadEntry[] => list.filter((d) => d.status === 'downloading')

/** Reloads the saved list without ever throwing. A download left "in progress" (app killed) becomes "failed". */
export function parseDownloads(raw: unknown): DownloadEntry[] {
  if (!Array.isArray(raw)) return []
  const out: DownloadEntry[] = []
  const seen = new Set<string>()
  for (const r of raw) {
    if (!r || typeof r !== 'object') continue
    const d = r as Record<string, unknown>
    if (typeof d.id !== 'string' || !d.id || seen.has(d.id) || typeof d.name !== 'string' || !d.name || typeof d.url !== 'string') continue
    seen.add(d.id)
    const status: DownloadStatus = d.status === 'done' ? 'done' : 'failed'
    const path = typeof d.path === 'string' ? d.path : ''
    out.push({
      id: d.id,
      name: d.name,
      url: d.url,
      size: typeof d.size === 'number' && Number.isFinite(d.size) && d.size > 0 ? d.size : 0,
      date: typeof d.date === 'number' && Number.isFinite(d.date) ? d.date : 0,
      status: status === 'done' && !path ? 'failed' : status,
      path
    })
  }
  return out.slice(0, DOWNLOADS_CAP)
}

/** What is saved: the list without progress. */
export const serializeDownloads = (list: readonly DownloadEntry[]): string => JSON.stringify(list.map(({ progress: _progress, ...rest }) => rest))

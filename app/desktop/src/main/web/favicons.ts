// Fallback favicons: fetched by the main process when the interface cannot load them itself.
import { session, type Session } from 'electron'
import type { Browser } from '../browser/browser'

/** Maximum size of a favicon, and number of entries kept in memory. */
const MAX_BYTES = 512 * 1024
const MAX_ENTRIES = 400
/** Total budget (characters of the data URLs) of the cache. */
const MAX_TOTAL = 32 * 1024 * 1024
/** A failure is retried after this delay (network back, connection made). */
const RETRY_MS = 60_000
const TIMEOUT_MS = 10_000

const cache = new Map<string, { data: string | null; at: number }>()
let total = 0
const pending = new Map<string, Promise<string | null>>()

/** .ico icons are often served with no type or as `application/octet-stream`: we look at the first bytes. */
function imageType(declared: string, bytes: Uint8Array): string | null {
  const type = declared.split(';')[0].trim().toLowerCase()
  if (type.startsWith('image/')) return type
  const head = Array.from(bytes.slice(0, 4))
  if (head[0] === 0 && head[1] === 0 && head[2] === 1 && head[3] === 0) return 'image/x-icon'
  if (head[0] === 0x89 && head[1] === 0x50) return 'image/png'
  if (head[0] === 0xff && head[1] === 0xd8) return 'image/jpeg'
  if (head[0] === 0x47 && head[1] === 0x49) return 'image/gif'
  if (head[0] === 0x52 && head[1] === 0x49) return 'image/webp'
  if (/^\s*<(\?xml|svg)/i.test(new TextDecoder().decode(bytes.slice(0, 64)))) return 'image/svg+xml'
  return null
}

async function fetchWith(ses: Session, url: string, credentials: 'include' | 'omit'): Promise<string | null> {
  try {
    // With the tab session's cookies: some sites only serve their icon to signed-in visitors.
    const res = await ses.fetch(url, { credentials, signal: AbortSignal.timeout(TIMEOUT_MS) })
    if (!res.ok) return null
    const bytes = new Uint8Array(await res.arrayBuffer())
    if (bytes.length === 0 || bytes.length > MAX_BYTES) return null
    const type = imageType(res.headers.get('content-type') ?? '', bytes)
    return type ? `data:${type};base64,${Buffer.from(bytes).toString('base64')}` : null
  } catch {
    return null
  }
}

/**
 * Favicon as a data URL, or null. Fetched with the session of the tab carrying this icon; with no tab (bookmarks, history)
 * or for a private tab, without cookies.
 * Successes stay in memory; failures are retried after a minute.
 */
export function fallbackFavicon(b: Browser, url: unknown): Promise<string | null> {
  if (typeof url !== 'string' || !/^https?:\/\//i.test(url) || url.length > 4096) return Promise.resolve(null)
  const hit = cache.get(url)
  if (hit && (hit.data || Date.now() - hit.at < RETRY_MS)) {
    cache.delete(url) // the most recently used entry goes to the end of the list
    cache.set(url, hit)
    return Promise.resolve(hit.data)
  }
  const running = pending.get(url)
  if (running) return running
  const job = (async () => {
    let ses: Session = session.defaultSession
    let credentials: 'include' | 'omit' = 'omit'
    let privateOnly = false
    const owners = Object.keys(b.store.state.tabs).filter((id) => b.store.state.tabs[id].favicon === url)
    const open = owners.find((id) => !b.privates.owns(id) && b.views.viewOf(id) && !b.views.viewOf(id)!.webContents.isDestroyed())
    if (open) {
      ses = b.views.viewOf(open)!.webContents.session
      credentials = 'include'
    } else privateOnly = owners.length > 0
    const data = await fetchWith(ses, url, credentials)
    if (hit) {
      cache.delete(url)
      total -= hit.data?.length ?? 0
    }
    if (!privateOnly) {
      cache.set(url, { data, at: Date.now() })
      total += data?.length ?? 0
      while (cache.size > MAX_ENTRIES || total > MAX_TOTAL) {
        const oldest = cache.keys().next().value!
        total -= cache.get(oldest)?.data?.length ?? 0
        cache.delete(oldest)
      }
    }
    return data
  })().finally(() => pending.delete(url))
  pending.set(url, job)
  return job
}

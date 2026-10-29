import { app } from 'electron'
import { copyFileSync, readFileSync } from 'node:fs'
import { join } from 'node:path'
import { AtomicWriter } from './atomic'
import { fold } from './urls'
import { historyScore } from './rank'

export interface HistoryEntry {
  url: string
  title: string
  favicon?: string
  visits: number
  /** Times the page was reached from the command bar (typed or picked), like Chrome's `typed_count`. Missing = 0. */
  typed?: number
  lastVisit: number
}

const MAX_ENTRIES = 5000
const IGNORED = /^(about|data|devtools):/i
const FAVICON_MAX = 200_000

/** Oversized `data:` icons are not kept in the history. */
const capIcon = (f?: string): string | undefined => (f && f.startsWith('data:') && f.length > FAVICON_MAX ? undefined : f)

export class History {
  // Insertion order follows recency: the first entry is the oldest.
  private entries = new Map<string, HistoryEntry>()
  // Folded keys (title, url) per entry, never written to disk.
  private keys = new WeakMap<HistoryEntry, { title: string; url: string }>()
  private timer: NodeJS.Timeout | null = null
  private file = join(app.getPath('userData'), 'history.json')
  /** Loaded on demand (see `load`): until then nothing is written, so an empty history never replaces the file. */
  private loaded = false
  /** Unreadable file (I/O error): it is not written during this session. */
  private locked = false
  private writer = new AtomicWriter(this.file, () => (this.loaded && !this.locked ? JSON.stringify([...this.entries.values()]) : null))

  /** Reads the file, only once; called at the top of every method that touches entries. */
  private load(): void {
    if (this.loaded) return
    this.loaded = true
    try {
      const list = JSON.parse(readFileSync(this.file, 'utf8')) as HistoryEntry[]
      for (const e of list) if (e && typeof e.url === 'string') this.entries.set(e.url, e)
    } catch (e) {
      const code = (e as NodeJS.ErrnoException).code
      if (e instanceof SyntaxError) {
        // Corrupt file: copy it aside before the next write replaces it.
        this.entries.clear()
        try {
          copyFileSync(this.file, `${this.file}.corrupt-${Date.now()}`)
        } catch {
          // no file to back up
        }
      } else if (code !== 'ENOENT') this.locked = true // unreadable, maybe intact: leave it alone
    }
  }

  /** Loads the history during idle time (the palette will not wait for the read). */
  preload(): void {
    this.load()
  }

  private key(e: HistoryEntry): { title: string; url: string } {
    let k = this.keys.get(e)
    if (!k) {
      k = { title: fold(e.title), url: fold(e.url) }
      this.keys.set(e, k)
    }
    return k
  }

  get size(): number {
    this.load()
    return this.entries.size
  }

  clear(): void {
    this.load()
    this.entries.clear()
    this.flush()
  }

  /** `typed`: the navigation was started from the command bar (see `typed-nav.ts`), which counts as a typed visit. */
  record(url: string, title: string, favicon?: string, typed = false): void {
    if (!url || IGNORED.test(url)) return
    this.load()
    const previous = this.entries.get(url)
    this.entries.delete(url)
    this.entries.set(url, {
      url,
      title: title || previous?.title || url,
      favicon: capIcon(favicon) ?? previous?.favicon,
      visits: (previous?.visits ?? 0) + 1,
      ...(previous?.typed || typed ? { typed: (previous?.typed ?? 0) + (typed ? 1 : 0) } : {}),
      lastVisit: Date.now()
    })
    if (this.entries.size > MAX_ENTRIES) {
      const oldest = this.entries.keys().next().value
      if (oldest !== undefined) this.entries.delete(oldest)
    }
    this.schedule()
  }

  /**
   * Adds another browser's history: an already known page keeps its most recent visit and accumulates visits.
   * Recency order is rebuilt, then only the 5,000 most recent pages are kept. Returns the number of new pages.
   */
  importEntries(list: { url: string; title: string; visits: number; typed?: number; lastVisit: number }[]): number {
    this.load()
    let added = 0
    for (const e of list) {
      if (!e.url || IGNORED.test(e.url) || !Number.isFinite(e.lastVisit)) continue
      const known = this.entries.get(e.url)
      if (known) {
        known.visits += Math.max(0, e.visits)
        if (e.typed && e.typed > 0) known.typed = (known.typed ?? 0) + e.typed
        if (e.lastVisit > known.lastVisit) known.lastVisit = e.lastVisit
        if ((!known.title || known.title === known.url) && e.title) {
          known.title = e.title
          this.keys.delete(known)
        }
        continue
      }
      const typed = e.typed && e.typed > 0 ? { typed: Math.floor(e.typed) } : {}
      this.entries.set(e.url, { url: e.url, title: e.title || e.url, visits: Math.max(1, e.visits), ...typed, lastVisit: e.lastVisit })
      added++
    }
    const sorted = [...this.entries.values()].sort((a, b) => a.lastVisit - b.lastVisit).slice(-MAX_ENTRIES)
    this.entries = new Map(sorted.map((e) => [e.url, e]))
    this.flush()
    return added
  }

  update(url: string, patch: { title?: string; favicon?: string }): void {
    this.load()
    const entry = this.entries.get(url)
    if (!entry) return
    let changed = false
    if (patch.title && patch.title !== entry.title) {
      entry.title = patch.title
      this.keys.delete(entry)
      changed = true
    }
    const icon = capIcon(patch.favicon)
    if (icon && icon !== entry.favicon) {
      entry.favicon = icon
      changed = true
    }
    if (changed) this.schedule(60_000) // title/icon only: deferred write (shutdown flushes anyway)
  }

  /** History page (most recent to oldest), filtered ignoring accents. */
  list(query: string, offset: number, limit: number): HistoryEntry[] {
    this.load()
    const q = fold(query.trim())
    const all = [...this.entries.values()]
    const out: HistoryEntry[] = []
    let skipped = 0
    for (let i = all.length - 1; i >= 0 && out.length < limit; i--) {
      const e = all[i]
      if (q) {
        const k = this.key(e)
        if (!k.title.includes(q) && !k.url.includes(q)) continue
      }
      if (skipped < offset) skipped++
      else out.push(e)
    }
    return out
  }

  /** The entry of exactly this URL, or undefined. */
  get(url: string): HistoryEntry | undefined {
    this.load()
    return this.entries.get(url)
  }

  remove(url: string): void {
    this.load()
    if (this.entries.delete(url)) this.schedule()
  }

  /** Entries whose title or URL contains the query, best first (match quality weighted by frecency, see `rank.ts`). */
  search(query: string, exclude: Set<string>, limit: number): HistoryEntry[] {
    this.load()
    const q = fold(query)
    const now = Date.now()
    // Selects the best `limit` by insertion; on equal score, insertion order is kept (stable sort).
    const top: { e: HistoryEntry; s: number }[] = []
    if (limit <= 0) return []
    for (const e of this.entries.values()) {
      if (exclude.has(e.url)) continue
      const k = this.key(e)
      const s = historyScore(q, { title: k.title, url: k.url, visits: e.visits, typed: e.typed, lastVisit: e.lastVisit }, now)
      if (!s) continue
      if (top.length === limit && s <= top[limit - 1].s) continue
      let i = top.length
      while (i > 0 && top[i - 1].s < s) i--
      top.splice(i, 0, { e, s })
      if (top.length > limit) top.pop()
    }
    return top.map((x) => x.e)
  }

  /** Synchronous write (shutdown, clearing). No effect if the history was never loaded: the file stays intact. */
  flush(): void {
    if (this.timer) clearTimeout(this.timer)
    this.timer = null
    this.writer.flushSync()
  }

  private timerDelay = 0

  private schedule(delay = 2000): void {
    if (this.timer) {
      if (delay >= this.timerDelay) return
      clearTimeout(this.timer) // a slow write is already planned: bring it forward
    }
    this.timerDelay = delay
    this.timer = setTimeout(() => {
      this.timer = null
      this.writer.request()
    }, delay)
  }
}

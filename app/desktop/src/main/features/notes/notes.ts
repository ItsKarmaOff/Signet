// Notes and Easels: one JSON file per document in userData/notes, opened as tabs (`signet://note/<id>`).
// The document is shown in the app's UI (route `#note/<id>` or `#easel/<id>`); its page script
// (src/preload/notes.ts) only calls the `notes:*` channels, and main only serves the document named by the address
// of the sending frame: a web page can neither read nor write a document.
import { app, ipcMain, type IpcMainInvokeEvent } from 'electron'
import { randomUUID } from 'node:crypto'
import { mkdirSync, readFileSync, unlinkSync } from 'node:fs'
import { join } from 'node:path'
import type { EaselDoc, EaselItem, NoteDoc, NoteKind, NotesDoc, NotesEnv, NotesPatch } from '@shared/notes'
import { EASEL_MAX, NOTE_ID, NOTE_MAX, noteAddress, parseNoteAddress } from '@shared/notes'
import type { Browser } from '../../browser/browser'
import { AtomicWriter } from '../../core/atomic'
import { appearanceOf } from '../../app/appearance'
import { currentLocale, t } from '../../app/i18n'
import { buildMenu, menuIcon } from '../../app/menuicons'
import { isAppUrl } from '../../core/urls'

const TITLE_MAX = 200

/** Real address of a document: the app's UI on the route `#note/<id>` or `#easel/<id>`. */
export function noteAppUrl(kind: NoteKind, id: string): string {
  const hash = `#${kind}/${id}`
  const dev = process.env.ELECTRON_RENDERER_URL
  return dev ? dev + hash : 'signetui://app/index.html' + hash
}

/** `signet://note/<id>` → real address to load; null for any other address. */
export function resolveNoteAddress(url: string): string | null {
  const doc = parseNoteAddress(url)
  return doc ? noteAppUrl(doc.kind, doc.id) : null
}

/** Document designated by the real address of a UI page, or null. */
export function noteOfAppUrl(url: string): { kind: NoteKind; id: string } | null {
  if (!isAppUrl(url)) return null
  const m = /#(note|easel)\/([0-9a-f-]{36})$/i.exec(url)
  return m && NOTE_ID.test(m[2]) ? { kind: m[1].toLowerCase() as NoteKind, id: m[2].toLowerCase() } : null
}

/** Finite bounded number, otherwise `fallback`. */
const num = (v: unknown, min: number, max: number, fallback: number) =>
  typeof v === 'number' && Number.isFinite(v) ? Math.min(max, Math.max(min, v)) : fallback

/** Easel items coming from the page, validated (types, bounds, known fields only). */
function cleanItems(value: unknown): EaselItem[] {
  if (!Array.isArray(value)) return []
  const out: EaselItem[] = []
  for (const raw of value.slice(0, 5000)) {
    if (!raw || typeof raw !== 'object') continue
    const v = raw as Partial<EaselItem>
    if (v.type !== 'text' && v.type !== 'image' && v.type !== 'stroke') continue
    const item: EaselItem = {
      id: typeof v.id === 'string' ? v.id.slice(0, 64) : randomUUID(),
      type: v.type,
      x: num(v.x, -1e6, 1e6, 0),
      y: num(v.y, -1e6, 1e6, 0),
      w: num(v.w, 1, 1e5, 100),
      h: num(v.h, 1, 1e5, 100)
    }
    if (typeof v.color === 'string' && /^(#[0-9a-f]{3,8}|var\(--[a-z0-9-]+\))$/i.test(v.color)) item.color = v.color
    if (v.type === 'text') item.html = typeof v.html === 'string' ? v.html : ''
    if (v.type === 'image') {
      if (typeof v.src !== 'string' || !/^data:image\/(png|jpeg|webp|gif);base64,/i.test(v.src)) continue
      item.src = v.src
    }
    if (v.type === 'stroke') {
      if (!Array.isArray(v.points) || v.points.length < 2) continue
      item.points = v.points.slice(0, 20000).map((p) => num(p, -1e6, 1e6, 0))
      item.vw = num(v.vw, 1, 1e5, item.w)
      item.vh = num(v.vh, 1, 1e5, item.h)
      item.size = num(v.size, 0.5, 64, 3)
    }
    out.push(item)
  }
  return out
}

/** Documents: read, write (atomic, one file per document), create, delete, page channels. */
export class Notes {
  private dir = join(app.getPath('userData'), 'notes')
  private docs = new Map<string, NotesDoc>()
  private writers = new Map<string, AtomicWriter>()

  constructor(private b: Browser) {}

  private file(id: string): string {
    return join(this.dir, `${id}.json`)
  }

  /** Document by its id (read from disk on first request), or null. */
  private read(id: string): NotesDoc | null {
    const cached = this.docs.get(id)
    if (cached) return cached
    try {
      const doc = JSON.parse(readFileSync(this.file(id), 'utf8')) as NotesDoc
      if (doc && doc.id === id && (doc.kind === 'note' || doc.kind === 'easel')) {
        this.docs.set(id, doc)
        return doc
      }
    } catch {
      // missing or unreadable: new document
    }
    return null
  }

  private write(doc: NotesDoc): void {
    this.docs.set(doc.id, doc)
    let writer = this.writers.get(doc.id)
    if (!writer) {
      mkdirSync(this.dir, { recursive: true })
      writer = new AtomicWriter(this.file(doc.id), () => {
        const d = this.docs.get(doc.id)
        return d ? JSON.stringify(d) : null
      })
      this.writers.set(doc.id, writer)
    }
    writer.request()
  }

  /** Writes everything pending (app shutdown). */
  flush(): void {
    for (const w of this.writers.values()) w.flushSync()
  }

  /** Creates a document (empty, or already filled `seed`) and opens it in a new tab of the active space. */
  create(kind: NoteKind, seed?: (id: string, now: number) => NotesDoc): string {
    const id = randomUUID()
    if (seed) this.write(seed(id, Date.now()))
    this.b.window.show()
    this.b.tabs.open(noteAddress(kind, id), { activate: true })
    return id
  }

  /** New Easel with an image (capture tool), scaled down to 900 px wide at most. */
  createEaselWithImage(png: string, width: number, height: number): void {
    const scale = Math.min(1, 900 / Math.max(width, 1))
    this.create('easel', (id, now) => ({
      id,
      kind: 'easel',
      title: '',
      items: cleanItems([{ id: randomUUID(), type: 'image', x: 60, y: 60, w: Math.round(width * scale), h: Math.round(height * scale), src: png }]),
      view: { x: 0, y: 0, zoom: 1 },
      createdAt: now,
      updatedAt: now
    }))
  }

  /** Deletes a document: its file, then its open tabs. */
  remove(kind: NoteKind, id: string): void {
    this.docs.delete(id)
    this.writers.get(id)?.cancel() // an in-progress write must not resurrect the file
    this.writers.delete(id)
    try {
      unlinkSync(this.file(id))
    } catch {
      // already absent
    }
    const address = noteAddress(kind, id)
    for (const tab of Object.values(this.b.store.state.tabs)) if (tab.url === address) this.b.tabs.close(tab.id)
  }

  /** "New" menu (right-click on "New tab"): tab, note, Easel. */
  showNewMenu(): void {
    buildMenu([
      { label: t('notes.menu.newTab'), icon: menuIcon('newTab'), click: () => this.b.overlay.openPalette('new-tab') },
      { type: 'separator' },
      { label: t('notes.menu.newNote'), click: () => this.create('note') },
      { label: t('notes.menu.newEasel'), click: () => this.create('easel') }
    ]).popup()
  }

  // ───────────────────────── Page channels ─────────────────────────

  /**
   * Sender's document: main frame of a tab, whose real address is the app's UI on the route
   * of a document. The id comes from this address (never from an argument), so a page only touches its own document.
   */
  private senderDoc(e: IpcMainInvokeEvent): { kind: NoteKind; id: string; tabId: string } | null {
    const tabId = this.b.views.tabIdOfContents(e.sender.id)
    const frame = e.senderFrame
    if (!tabId || !frame || frame.parent) return null
    const doc = noteOfAppUrl(frame.url)
    return doc ? { ...doc, tabId } : null
  }

  registerIpc(): void {
    const guarded = <A extends unknown[], R>(channel: string, fn: (doc: { kind: NoteKind; id: string; tabId: string }, ...args: A) => R) =>
      ipcMain.handle(channel, (e, ...args) => {
        const doc = this.senderDoc(e)
        if (!doc) throw new Error('Call refused')
        return fn(doc, ...(args as A))
      })
    guarded('notes:locale', () => currentLocale())
    guarded('notes:appearance', () => appearanceOf(this.b.store.state.settings))
    guarded('notes:load', ({ kind, id, tabId }): NotesEnv => {
      const doc = this.read(id)
      const hue = this.b.model.locate(tabId)?.space?.hue ?? this.b.space.hue
      return { kind, id, doc: doc && doc.kind === kind ? doc : null, hue }
    })
    guarded('notes:save', ({ kind, id }, patch: NotesPatch): boolean => this.save(kind, id, patch))
    guarded('notes:remove', ({ kind, id }) => this.remove(kind, id))
  }

  private save(kind: NoteKind, id: string, patch: NotesPatch): boolean {
    if (!patch || typeof patch !== 'object') return false
    const title = typeof patch.title === 'string' ? patch.title.trim().slice(0, TITLE_MAX) : ''
    const prev = this.read(id)
    const now = Date.now()
    const createdAt = prev?.createdAt ?? now
    if (kind === 'note') {
      const html = 'html' in patch && typeof patch.html === 'string' ? patch.html : ''
      if (html.length > NOTE_MAX) return false
      const doc: NoteDoc = { id, kind, title, html, createdAt, updatedAt: now }
      this.write(doc)
      return true
    }
    if (!('items' in patch)) return false
    const v = patch.view
    const doc: EaselDoc = {
      id,
      kind,
      title,
      items: cleanItems(patch.items),
      view: { x: num(v?.x, -1e7, 1e7, 0), y: num(v?.y, -1e7, 1e7, 0), zoom: num(v?.zoom, 0.1, 8, 1) },
      createdAt,
      updatedAt: now
    }
    if (JSON.stringify(doc).length > EASEL_MAX) return false
    this.write(doc)
    return true
  }
}

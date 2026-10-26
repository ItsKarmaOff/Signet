// Notes and Easels: documents opened like tabs (`signet://note/<id>`, `signet://easel/<id>`).
// Contract between main (src/main/features/notes/notes.ts), the page script (src/preload/notes.ts) and the UI (Note.tsx, Easel.tsx).
import type { Appearance, Locale } from './types'

export type NoteKind = 'note' | 'easel'

/** Document identifier (UUID). */
export const NOTE_ID = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i

/** Displayed address of a document: `signet://note/<id>` or `signet://easel/<id>`. */
export const noteAddress = (kind: NoteKind, id: string) => `signet://${kind}/${id}`

/** `signet://note/<id>` → { kind, id }; null for any other address. */
export function parseNoteAddress(url: string): { kind: NoteKind; id: string } | null {
  const m = /^signet:\/\/(note|easel)\/([0-9a-f-]{36})\/?$/i.exec(url.trim())
  return m && NOTE_ID.test(m[2]) ? { kind: m[1].toLowerCase() as NoteKind, id: m[2].toLowerCase() } : null
}

/** Note: rich text (sanitized HTML). */
export interface NoteDoc {
  id: string
  kind: 'note'
  title: string
  html: string
  createdAt: number
  updatedAt: number
}

/** Easel item, in board coordinates (px at zoom 1). */
export interface EaselItem {
  id: string
  type: 'text' | 'image' | 'stroke'
  x: number
  y: number
  w: number
  h: number
  /** Text: sanitized HTML. */
  html?: string
  /** Image: data URL (downscaled to 1600 px per side). */
  src?: string
  /** Stroke: points [x0, y0, x1, y1…] relative to the item's corner, in a `vw` × `vh` frame. */
  points?: number[]
  vw?: number
  vh?: number
  /** Stroke or text color (CSS). */
  color?: string
  /** Stroke thickness (px at zoom 1). */
  size?: number
}

/** Easel: free-form board (texts, images, drawings), with the view (pan, zoom) where it was left. */
export interface EaselDoc {
  id: string
  kind: 'easel'
  title: string
  items: EaselItem[]
  view: { x: number; y: number; zoom: number }
  createdAt: number
  updatedAt: number
}

export type NotesDoc = NoteDoc | EaselDoc

/** What a document receives on opening: its content (null if new) and its space's tint. */
export interface NotesEnv {
  kind: NoteKind
  id: string
  doc: NotesDoc | null
  hue: number
}

/** Record: note (title + HTML) or Easel (title + items + view). */
export type NotesPatch = { title: string; html: string } | { title: string; items: EaselItem[]; view: EaselDoc['view'] }

/** Page script API, exposed only to document pages (`window.signetNotes`). */
export interface SignetNotesApi {
  load(): Promise<NotesEnv>
  /** False if the content is refused (too large). */
  save(patch: NotesPatch): Promise<boolean>
  /** Deletes the document and closes its tabs. */
  remove(): Promise<void>
}

/** Part of `window.signet` that the page script provides to documents (UI language and appearance). */
export interface NotesSignetSubset {
  getLocale(): Promise<Locale>
  onLocale(cb: (l: Locale) => void): () => void
  getAppearance(): Promise<Appearance>
  onAppearance(cb: (a: Appearance) => void): () => void
}

/** Maximum size of a note (HTML) and of an Easel (JSON, images included). */
export const NOTE_MAX = 2 * 1024 * 1024
export const EASEL_MAX = 30 * 1024 * 1024

declare global {
  interface Window {
    signetNotes?: SignetNotesApi
  }
}

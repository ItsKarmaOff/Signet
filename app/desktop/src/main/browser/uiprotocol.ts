// App UI served at `signetui://app/…` (production) rather than file://: extension content scripts
// (file access) do not inject into a custom scheme, hence never into the UI (passwords, buttons…).
import { join, relative, resolve, isAbsolute, sep } from 'node:path'
import { pathToFileURL } from 'node:url'
import { net, type Session } from 'electron'

export const UI_SCHEME = 'signetui'
const ROOT = join(__dirname, '../renderer')
// Embedding destinations: the UI never loads inside another page's frame.
const FRAMED = new Set(['iframe', 'frame', 'embed', 'object'])

/** Serves `signetui://app/<path>` from the UI folder, on a session. */
export function attachUiProtocol(ses: Session): void {
  if (process.env.ELECTRON_RENDERER_URL) return // development: Vite server
  ses.protocol.handle(UI_SCHEME, async (req) => {
    if (req.method !== 'GET') return new Response('', { status: 405 })
    if (FRAMED.has(req.headers.get('sec-fetch-dest') ?? '')) return new Response('', { status: 403 })
    try {
      const u = new URL(req.url)
      if (u.host !== 'app') return new Response('', { status: 404 })
      const path = decodeURIComponent(u.pathname)
      if (path.includes('\0')) return new Response('', { status: 404 })
      const file = resolve(ROOT, '.' + (path === '/' ? '/index.html' : path))
      const rel = relative(ROOT, file)
      if (!rel || rel === '..' || rel.startsWith('..' + sep) || isAbsolute(rel)) return new Response('', { status: 404 })
      return await net.fetch(pathToFileURL(file).toString())
    } catch {
      return new Response('', { status: 404 })
    }
  })
}

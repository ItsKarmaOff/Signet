// Development servers on the computer: listening TCP ports, kept if they answer over HTTP (page title read along the way),
// plus tabs open on a local address. The list goes to connected phones (`servers` message).
import { execFile } from 'node:child_process'
import { readFile } from 'node:fs/promises'
import { request } from 'node:http'
import { isLoopbackUrl, loopbackPort, toLinkLocalhost } from '@signet/shared/link/loopback'
import type { DevServer } from '@signet/shared/link/protocol'

/** Ports that are never a development server (SSH, DNS, file sharing, databases, printing, VNC…). */
const IGNORED_PORTS = new Set([
  22, 25, 53, 88, 111, 135, 137, 138, 139, 389, 445, 465, 548, 587, 631, 993, 995, 1433, 3306, 3389, 5353, 5432, 5900, 6379, 11211, 27017
])
/** Start of the ephemeral port range (IANA, macOS, Windows): apps, not dev servers. */
const EPHEMERAL_START = 49152
const MAX_SERVERS = 64
/** A port already recognized as HTTP is re-read (title) at this frequency; another one, less often. */
const REFRESH_HTTP_MS = 30_000
const REFRESH_OTHER_MS = 60_000
const PROBE_MS = 1000
const PROBE_BYTES = 16 * 1024
const PROBE_PARALLEL = 8
const LIST_TIMEOUT_MS = 4000

// ───────────────────────── Listening ports ─────────────────────────

const portOf = (addr: string): number | null => {
  const m = /:(\d{1,5})$/.exec(addr)
  const n = m ? Number(m[1]) : NaN
  return n > 0 && n < 65536 ? n : null
}

const unique = (ports: (number | null)[]): number[] => [...new Set(ports.filter((p): p is number => p !== null))].sort((a, b) => a - b)

/** macOS: `lsof -nP -iTCP -sTCP:LISTEN`. */
export function parseLsof(text: string): number[] {
  return unique(
    text.split('\n').map((l) => {
      const m = /\sTCP\s+(\S+)\s+\(LISTEN\)/.exec(l)
      return m ? portOf(m[1]!) : null
    })
  )
}

/** Linux : `ss -ltnH` (adresse locale en 4e colonne). */
export function parseSs(text: string): number[] {
  return unique(text.split('\n').map((l) => portOf(l.trim().split(/\s+/)[3] ?? '')))
}

/** Windows: `netstat -ano -p TCP`. The word "LISTENING" is translated: we recognize the remote address `…:0` instead. */
export function parseNetstat(text: string): number[] {
  return unique(
    text.split('\n').map((l) => {
      const c = l.trim().split(/\s+/)
      return c[0]?.toUpperCase() === 'TCP' && /:0$/.test(c[2] ?? '') ? portOf(c[1] ?? '') : null
    })
  )
}

/** Linux, without `ss`: `/proc/net/tcp` and `tcp6` (state `0A` = listening, port in hexadecimal). */
export function parseProcNet(text: string): number[] {
  return unique(
    text.split('\n').map((l) => {
      const c = l.trim().split(/\s+/)
      if (c[3] !== '0A') return null
      const hex = /:([0-9A-Fa-f]{4})$/.exec(c[1] ?? '')
      return hex ? parseInt(hex[1]!, 16) : null
    })
  )
}

function run(cmd: string, args: string[]): Promise<string> {
  return new Promise((resolve, reject) => {
    execFile(cmd, args, { timeout: LIST_TIMEOUT_MS, maxBuffer: 4 * 1024 * 1024, windowsHide: true }, (err, stdout) => {
      // `lsof` returns 1 when nothing is listening: the output counts, not the code.
      if (err && !stdout) reject(err)
      else resolve(stdout)
    })
  })
}

/** TCP ports listening on this machine. Empty array if the system command is missing. */
export async function listListeningPorts(platform: NodeJS.Platform = process.platform): Promise<number[]> {
  try {
    if (platform === 'darwin') return parseLsof(await run('/usr/sbin/lsof', ['-nP', '-iTCP', '-sTCP:LISTEN']))
    if (platform === 'win32') return parseNetstat(await run('netstat', ['-ano', '-p', 'TCP']))
    try {
      return parseSs(await run('ss', ['-ltnH']))
    } catch {
      const files = await Promise.all(['/proc/net/tcp', '/proc/net/tcp6'].map((f) => readFile(f, 'utf8').catch(() => '')))
      return unique(files.flatMap((f) => parseProcNet(f)))
    }
  } catch {
    return []
  }
}

// ───────────────────────── Sonde HTTP ─────────────────────────

export interface ProbeResult {
  /** The port answers with an HTML page (a JSON API or an app service does not count). */
  http: boolean
  title?: string
}

const decode = (s: string): string =>
  s
    .replace(/&amp;/g, '&')
    .replace(/&lt;/g, '<')
    .replace(/&gt;/g, '>')
    .replace(/&quot;/g, '"')
    .replace(/&#39;|&apos;/g, "'")

/** Content of the `<title>` tag, on one line, or undefined. */
export function extractTitle(html: string): string | undefined {
  const m = /<title[^>]*>([\s\S]*?)<\/title>/i.exec(html)
  const title = m ? decode(m[1]!).replace(/\s+/g, ' ').trim() : ''
  return title ? title.slice(0, 200) : undefined
}

function probeHost(host: string, port: number): Promise<ProbeResult | null> {
  return new Promise((resolve) => {
    let done = false
    const finish = (r: ProbeResult | null) => {
      if (done) return
      done = true
      req.destroy()
      resolve(r)
    }
    const req = request(
      {
        host,
        port,
        path: '/',
        method: 'GET',
        agent: false,
        timeout: PROBE_MS,
        headers: { host: `localhost:${port}`, accept: 'text/html,*/*;q=0.5', connection: 'close' }
      },
      (res) => {
        // The macOS AirPlay service (ports 5000 and 7000) answers over HTTP but is not a page.
        if (/^AirTunes/i.test(String(res.headers.server ?? ''))) return finish(null)
        const type = String(res.headers['content-type'] ?? '')
        if (!/html/i.test(type)) return finish({ http: false })
        const chunks: Buffer[] = []
        let size = 0
        res.on('data', (c: Buffer) => {
          chunks.push(c)
          size += c.length
          const text = Buffer.concat(chunks).toString('utf8')
          if (size >= PROBE_BYTES || /<\/title>/i.test(text)) finish({ http: true, title: extractTitle(text) })
        })
        res.on('end', () => finish({ http: true, title: extractTitle(Buffer.concat(chunks).toString('utf8')) }))
        res.on('error', () => finish({ http: true }))
      }
    )
    req.on('timeout', () => finish(null))
    req.on('error', () => finish(null))
    req.end()
  })
}

/** Requests `/` over HTTP on 127.0.0.1 then ::1 (Vite often listens on ::1 only). */
export async function probePort(port: number): Promise<ProbeResult> {
  for (const host of ['127.0.0.1', '::1']) {
    const r = await probeHost(host, port)
    if (r) return r
  }
  return { http: false }
}

// ───────────────────────── Recherche ─────────────────────────

export interface TabLike {
  url: string
  title: string
}

export interface ScannerOptions {
  /** Port of the link itself, never to be offered. */
  ownPort: () => number | null
  tabs: () => TabLike[]
  list?: () => Promise<number[]>
  probe?: (port: number) => Promise<ProbeResult>
  now?: () => number
}

/** Detects dev servers. `scan()` returns the full list; probe results are kept for a while. */
export class DevServerScanner {
  private cache = new Map<number, { at: number; result: ProbeResult }>()
  private running: Promise<DevServer[]> | null = null
  /** All ports listening at the last sweep (pages, APIs…), plus those of local tabs: the phone relays them. */
  ports: number[] = []
  private list: () => Promise<number[]>
  private probe: (port: number) => Promise<ProbeResult>
  private now: () => number

  constructor(private o: ScannerOptions) {
    this.list = o.list ?? (() => listListeningPorts())
    this.probe = o.probe ?? probePort
    this.now = o.now ?? Date.now
  }

  scan(): Promise<DevServer[]> {
    // One sweep at a time.
    return (this.running ??= this.run().finally(() => (this.running = null)))
  }

  private async run(): Promise<DevServer[]> {
    const own = this.o.ownPort()
    const ports = (await this.list()).filter((p) => p !== own && p < EPHEMERAL_START && !IGNORED_PORTS.has(p))
    const now = this.now()
    const stale = ports.filter((p) => {
      const c = this.cache.get(p)
      return !c || now - c.at >= (c.result.http ? REFRESH_HTTP_MS : REFRESH_OTHER_MS)
    })
    for (let i = 0; i < stale.length; i += PROBE_PARALLEL) {
      await Promise.all(stale.slice(i, i + PROBE_PARALLEL).map(async (p) => this.cache.set(p, { at: now, result: await this.probe(p) })))
    }
    for (const p of [...this.cache.keys()]) if (!ports.includes(p)) this.cache.delete(p)
    const tabPorts = this.o.tabs().flatMap((t) => (isLoopbackUrl(t.url) ? [loopbackPort(t.url)] : []))
    this.ports = [...new Set([...ports, ...tabPorts])].filter((p): p is number => p !== null && p !== own).sort((a, b) => a - b)
    return this.combine(
      ports.flatMap((port) => {
        const r = this.cache.get(port)?.result
        return r?.http ? [{ port, title: r.title }] : []
      })
    )
  }

  /** Local tabs first (exact address, tab title), then ports that listen without a tab. */
  private combine(listening: { port: number; title?: string }[]): DevServer[] {
    const own = this.o.ownPort()
    const out: DevServer[] = []
    const seen = new Set<string>()
    const tabPorts = new Set<number>()
    for (const tab of this.o.tabs()) {
      const port = loopbackPort(tab.url)
      if (port === null || port === own || !isLoopbackUrl(tab.url)) continue
      const url = toLinkLocalhost(tab.url)
      if (seen.has(url)) continue
      seen.add(url)
      tabPorts.add(port)
      const title = tab.title.trim().slice(0, 200)
      out.push({ url, port, ...(title && { title }), fromTab: true })
    }
    for (const s of listening) {
      if (tabPorts.has(s.port)) continue
      out.push({ url: `http://localhost:${s.port}/`, port: s.port, ...(s.title && { title: s.title }) })
    }
    return out.slice(0, MAX_SERVERS)
  }
}

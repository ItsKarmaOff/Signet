import { createServer as createHttp, type Server } from 'node:http'
import { createServer as createTcp } from 'node:net'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { DevServerScanner, extractTitle, parseLsof, parseNetstat, parseProcNet, parseSs, probePort } from './devservers'
import { cleanHostName } from './identity'

describe('listening ports', () => {
  it('lsof (macOS)', () => {
    const out = `COMMAND   PID USER   FD   TYPE DEVICE SIZE/OFF NODE NAME
node    41234 chris   23u  IPv6 0x1234      0t0  TCP [::1]:5173 (LISTEN)
node    41235 chris   24u  IPv4 0x1235      0t0  TCP 127.0.0.1:3000 (LISTEN)
rapportd  612 chris    8u  IPv4 0x1236      0t0  TCP *:49152 (LISTEN)
rapportd  612 chris    9u  IPv6 0x1237      0t0  TCP *:49152 (LISTEN)
Safari    700 chris   10u  IPv4 0x1238      0t0  TCP 192.168.1.2:50000->1.2.3.4:443 (ESTABLISHED)`
    expect(parseLsof(out)).toEqual([3000, 5173, 49152])
  })

  it('ss (Linux)', () => {
    const out = `LISTEN 0 511 127.0.0.1:3000 0.0.0.0:*
LISTEN 0 4096 [::1]:5173 [::]:*
LISTEN 0 128 0.0.0.0:22 0.0.0.0:*
LISTEN 0 128 *:8080 *:*`
    expect(parseSs(out)).toEqual([22, 3000, 5173, 8080])
  })

  it('netstat (Windows), whatever the language', () => {
    const out = `
Connexions actives

  Proto  Adresse locale         Adresse distante       État
  TCP    0.0.0.0:135            0.0.0.0:0              LISTENING       1032
  TCP    127.0.0.1:5173         0.0.0.0:0              ABHÖREN         4412
  TCP    [::]:3000              [::]:0                 LISTENING       4412
  TCP    192.168.1.5:50123      52.1.2.3:443           ESTABLISHED     8000`
    expect(parseNetstat(out)).toEqual([135, 3000, 5173])
  })

  it('/proc/net/tcp (Linux without ss)', () => {
    const out = `  sl  local_address rem_address   st tx_queue rx_queue tr tm->when retrnsmt   uid  timeout inode
   0: 0100007F:1F90 00000000:0000 0A 00000000:00000000 00:00000000 00000000  1000        0 12345 1
   1: 0100007F:C350 0100007F:1F90 01 00000000:00000000 00:00000000 00000000  1000        0 12346 1`
    expect(parseProcNet(out)).toEqual([8080])
  })
})

describe('titre', () => {
  it('reads the title tag, on one line', () => {
    expect(extractTitle('<html><head><TITLE>\n  Mon  app &amp; co \n</TITLE>')).toBe('Mon app & co')
    expect(extractTitle('<html><body>rien</body>')).toBeUndefined()
    expect(extractTitle('<title> </title>')).toBeUndefined()
  })
})

describe('sonde HTTP', () => {
  const servers: { close: () => void }[] = []
  afterEach(() => servers.splice(0).forEach((s) => s.close()))
  const listen = (server: Server | ReturnType<typeof createTcp>): Promise<number> =>
    new Promise((resolve) => {
      servers.push(server)
      server.listen(0, '127.0.0.1', () => resolve((server.address() as { port: number }).port))
    })

  it('recognizes an HTTP server and reads its title', async () => {
    const port = await listen(createHttp((_q, res) => res.setHeader('content-type', 'text/html').end('<html><title>Vite App</title></html>')))
    expect(await probePort(port)).toEqual({ http: true, title: 'Vite App' })
  })

  it('a response that is not a page (JSON API) does not count', async () => {
    const port = await listen(createHttp((_q, res) => res.setHeader('content-type', 'application/json').end('{}')))
    expect(await probePort(port)).toEqual({ http: false })
  })

  it('ignores the macOS AirPlay service', async () => {
    const port = await listen(createHttp((_q, res) => res.writeHead(403, { server: 'AirTunes/860.7.1' }).end()))
    expect((await probePort(port)).http).toBe(false)
  })

  it('a port that is not HTTP is discarded', async () => {
    const port = await listen(createTcp((s) => s.end('SSH-2.0-pas-du-http\r\n')))
    expect((await probePort(port)).http).toBe(false)
    const silent = await listen(createTcp(() => {}))
    expect((await probePort(silent)).http).toBe(false)
  })
})

describe('DevServerScanner', () => {
  const make = (ports: number[], tabs: { url: string; title: string }[] = [], now = { t: 0 }) => {
    const probe = vi.fn(async (port: number) => (port === 5173 ? { http: true, title: 'Vite' } : port === 3000 ? { http: true } : { http: false }))
    const scanner = new DevServerScanner({ ownPort: () => 47615, tabs: () => tabs, list: async () => ports, probe, now: () => now.t })
    return { scanner, probe, now }
  }

  it('keeps HTTP servers, without the link port, system ports or ephemeral ports', async () => {
    const { scanner, probe } = make([22, 3000, 5173, 9999, 47615, 49182])
    expect(await scanner.scan()).toEqual([
      { url: 'http://localhost:3000/', port: 3000 },
      { url: 'http://localhost:5173/', port: 5173, title: 'Vite' }
    ])
    expect(probe.mock.calls.map((c) => c[0]).sort()).toEqual([3000, 5173, 9999])
  })

  it('local tabs come first, mapped to localhost; no duplicate with their port', async () => {
    const { scanner } = make(
      [5173, 3000],
      [
        { url: 'http://127.0.0.1:5173/app?x=1', title: 'Mon app' },
        { url: 'http://[::1]:5173/app?x=1', title: 'Doublon' },
        { url: 'https://exemple.com/', title: 'Dehors' },
        { url: 'http://localhost:47615/', title: 'Le lien lui-même' }
      ]
    )
    expect(await scanner.scan()).toEqual([
      { url: 'http://localhost:5173/app?x=1', port: 5173, title: 'Mon app', fromTab: true },
      { url: 'http://localhost:3000/', port: 3000 }
    ])
  })

  it('does not probe a recent port twice; re-reads after the delay', async () => {
    const { scanner, probe, now } = make([5173])
    await scanner.scan()
    now.t += 5000
    await scanner.scan()
    expect(probe).toHaveBeenCalledTimes(1)
    now.t += 30_000
    await scanner.scan()
    expect(probe).toHaveBeenCalledTimes(2)
  })
})

describe('computer name', () => {
  it('removes the network suffix and control characters', () => {
    expect(cleanHostName('MacBook-de-Christophe.local')).toBe('MacBook-de-Christophe')
    expect(cleanHostName('  PC\u0000 de\nLéa ')).toBe('PC deLéa')
    expect(cleanHostName('')).toBe('Signet')
    expect(cleanHostName('x'.repeat(100))).toHaveLength(64)
  })
})

#!/usr/bin/env node
// Fake Signet computer to test the native `signet-link` module without the desktop app (protocol: packages/shared/src/link).
//   node scripts/link-test-desktop.mjs [--port 47615] [--map 4321=5173] [--wrong-pin] [--no-mdns] [--host 192.168.1.5]
// Prints the pairing URL (signet://link?d=...) and logs everything. `--map` redirects the tunnel ports (the simulator shares the
// Mac's localhost: without it, localhost:4321 cannot prove traffic went through the tunnel).
import crypto from 'node:crypto'
import net from 'node:net'
import os from 'node:os'
import tls from 'node:tls'
import { Bonjour } from 'bonjour-service'
import { generate } from 'selfsigned'

const args = process.argv.slice(2)
const opt = (name) => {
  const i = args.indexOf(`--${name}`)
  return i >= 0 ? args[i + 1] : undefined
}
const flag = (name) => args.includes(`--${name}`)
const portMap = new Map(args.flatMap((a, i) => (args[i - 1] === '--map' ? [a.split('=').map(Number)] : [])))
const listenPort = Number(opt('port') ?? 47615)

const b64u = (buf) => Buffer.from(buf).toString('base64url')
const log = (...a) => console.log(new Date().toISOString().slice(11, 23), ...a)

const desktop = { id: 'test-desktop-' + b64u(crypto.randomBytes(4)), name: `Test Mac ${crypto.randomBytes(2).toString('hex')}`, platform: 'macos' }
const linkId = crypto.randomBytes(8).toString('hex')
const code = b64u(crypto.randomBytes(16))
let token = null // created at pairing
let codeUsed = false
const expiresAt = Date.now() + 5 * 60_000

async function makeCert(cn) {
  const pems = await generate([{ name: 'commonName', value: cn }], { keyType: 'ec', curve: 'P-256', days: 30, algorithm: 'sha256' })
  const der = new crypto.X509Certificate(pems.cert).raw
  return { key: pems.private, cert: pems.cert, sha: b64u(crypto.createHash('sha256').update(der).digest()) }
}
const linkCert = await makeCert('link ' + linkId)
const defaultCert = await makeCert('default')
const ctxFor = (c) => tls.createSecureContext({ key: c.key, cert: c.cert })
const ctxLink = ctxFor(linkCert)
const ctxDefault = ctxFor(defaultCert)

const send = (sock, msg) => sock.write(JSON.stringify(msg) + '\n')
const sameToken = (a) => token && typeof a === 'string' && a.length === token.length && crypto.timingSafeEqual(Buffer.from(a), Buffer.from(token))

function connectLoopback(port, cb) {
  // 127.0.0.1 then ::1 (Vite often listens on ::1 only)
  const tryHost = (host, next) => {
    const s = net.connect({ host, port })
    s.once('connect', () => cb(null, s))
    s.once('error', (e) => (next ? next() : cb(e)))
  }
  tryHost('127.0.0.1', () => tryHost('::1', null))
}

const server = tls.createServer(
  {
    minVersion: 'TLSv1.2',
    SNICallback: (name, cb) => {
      log('SNI', name)
      cb(null, name === `${linkId}.link.signet.invalid` ? ctxLink : ctxDefault)
    }
  },
  (sock) => {
    const peer = `${sock.remoteAddress}:${sock.remotePort}`
    log('connexion TLS', peer, sock.servername)
    let buf = Buffer.alloc(0)
    let role = null
    sock.on('error', (e) => log('socket error', peer, e.code ?? e.message))
    sock.on('close', () => log('socket closed', peer, role ?? ''))
    const onData = (chunk) => {
      buf = Buffer.concat([buf, chunk])
      if (role) return
      const nl = buf.indexOf(10)
      if (nl < 0) {
        if (buf.length > 65536) sock.destroy()
        return
      }
      const line = buf.subarray(0, nl).toString('utf8')
      const rest = buf.subarray(nl + 1)
      buf = Buffer.alloc(0)
      sock.off('data', onData)
      first(line, rest)
    }
    sock.on('data', onData)

    function first(line, rest) {
      log('<-', peer, line)
      let m
      try {
        m = JSON.parse(line)
      } catch {
        send(sock, { type: 'error', code: 'bad-request' })
        return sock.end()
      }
      if (m.v !== 1) {
        send(sock, { type: 'error', code: 'version' })
        return sock.end()
      }
      if (m.type === 'pair') {
        role = 'pair'
        if (m.linkId !== linkId) return reply({ type: 'error', code: 'unknown-link' })
        if (Date.now() > expiresAt) return reply({ type: 'error', code: 'expired' })
        const ok = !codeUsed && m.code?.length === code.length && crypto.timingSafeEqual(Buffer.from(m.code), Buffer.from(code))
        if (!ok) return reply({ type: 'error', code: 'bad-code' })
        codeUsed = true
        token = b64u(crypto.randomBytes(32))
        log('PAIRED with', JSON.stringify(m.device), 'jeton', token)
        return reply({ type: 'paired', token, device: desktop })
      }
      if (m.type === 'hello') {
        if (m.linkId !== linkId || !token) return reply({ type: 'error', code: 'unknown-link' }, (role = m.role))
        if (!sameToken(m.token)) return reply({ type: 'error', code: 'bad-token' }, (role = m.role))
        if (m.role === 'control') {
          role = 'control'
          send(sock, { type: 'welcome', device: desktop, paused: false })
          send(sock, { type: 'servers', servers: [{ url: 'http://localhost:5173', port: 5173, title: 'Vite', fromTab: true }] })
          let more = Buffer.from(rest)
          const feed = (c) => {
            more = Buffer.concat([more, c])
            let i
            while ((i = more.indexOf(10)) >= 0) {
              const l = more.subarray(0, i).toString('utf8')
              more = more.subarray(i + 1)
              log('<- control', l)
              try {
                const msg = JSON.parse(l)
                if (msg.type === 'ping') send(sock, { type: 'pong' })
                if (msg.type === 'open') log('open requested', msg.url)
              } catch {
                // line ignored
              }
            }
          }
          sock.on('data', feed)
          feed(Buffer.alloc(0))
          // Pushes an `open` after 3 s to check the computer -> phone direction
          setTimeout(() => !sock.destroyed && send(sock, { type: 'open', url: 'http://localhost:5173/' }), 3000)
          return
        }
        if (m.role === 'tunnel') {
          role = 'tunnel'
          const target = portMap.get(m.port) ?? m.port
          return connectLoopback(target, (err, up) => {
            if (err) {
              log('tunnel refused', m.port, '->', target, err.code)
              return reply({ type: 'error', code: 'port-closed' })
            }
            log('tunnel', m.port, '->', target)
            send(sock, { type: 'ok' })
            if (rest.length) up.write(rest)
            sock.pipe(up)
            up.pipe(sock)
            up.on('error', () => sock.destroy())
            sock.on('error', () => up.destroy())
            sock.on('close', () => up.destroy())
            up.on('close', () => sock.end())
          })
        }
      }
      reply({ type: 'error', code: 'bad-request' })
    }
    function reply(msg) {
      log('->', peer, JSON.stringify(msg))
      send(sock, msg)
      sock.end()
    }
  }
)

server.on('tlsClientError', (e) => log('TLS client error:', e.code ?? e.message))
await new Promise((r) => server.listen(listenPort, '0.0.0.0', r))
const port = server.address().port

const lan = Object.values(os.networkInterfaces())
  .flat()
  .filter((i) => i && i.family === 'IPv4' && !i.internal)
  .map((i) => i.address)
const hosts = opt('host') ? [opt('host')] : [...lan, '127.0.0.1']
const offer = {
  v: 1,
  desktop,
  linkId,
  certSha256: flag('wrong-pin') ? b64u(crypto.randomBytes(32)) : linkCert.sha,
  code,
  hosts,
  port,
  expiresAt
}
console.log('\nPairing URL:\n' + 'signet://link?d=' + Buffer.from(JSON.stringify(offer)).toString('base64url') + '\n')
log(
  'listening on',
  port,
  'hosts',
  hosts.join(','),
  'fingerprint',
  linkCert.sha,
  portMap.size ? 'map ' + [...portMap].map(([a, b]) => `${a}=${b}`).join(',') : ''
)

let bonjour
if (!flag('no-mdns')) {
  bonjour = new Bonjour()
  bonjour.publish({ name: desktop.name, type: 'signet', protocol: 'tcp', port, txt: { v: '1', id: desktop.id } })
  log('mDNS published _signet._tcp', desktop.name, 'id', desktop.id)
}
process.on('SIGINT', () => {
  bonjour?.unpublishAll(() => process.exit(0))
  setTimeout(() => process.exit(0), 500)
})

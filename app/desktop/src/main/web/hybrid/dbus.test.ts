import { mkdtempSync, rmSync } from 'node:fs'
import net from 'node:net'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { afterEach, describe, expect, it } from 'vitest'
import { DBusError, connect, parseBusAddress, systemBusPath } from './dbus'
import { Variant, decodeMessage, encodeMessage, messageLength, type Message } from './dbus-wire'

describe('parseBusAddress', () => {
  it('reads unix path and abstract sockets, skips other transports', () => {
    expect(parseBusAddress('unix:path=/run/dbus/system_bus_socket')).toBe('/run/dbus/system_bus_socket')
    expect(parseBusAddress('unix:path=/run/a%20b,guid=abc')).toBe('/run/a b')
    expect(parseBusAddress('unix:abstract=/tmp/dbus-x,guid=1')).toBe('\0/tmp/dbus-x')
    expect(parseBusAddress('tcp:host=localhost,port=1;unix:path=/x')).toBe('/x')
    expect(parseBusAddress('tcp:host=localhost,port=1')).toBeNull()
    expect(parseBusAddress('')).toBeNull()
    expect(parseBusAddress('unix:guid=1')).toBeNull()
  })

  it('uses the environment, else the default socket', () => {
    expect(systemBusPath({ DBUS_SYSTEM_BUS_ADDRESS: 'unix:path=/z' })).toBe('/z')
    expect(systemBusPath({})).toBe('/var/run/dbus/system_bus_socket')
    expect(systemBusPath({ DBUS_SYSTEM_BUS_ADDRESS: 'tcp:host=a' })).toBeNull()
  })
})

/** A bus that speaks just enough: auth, Hello, and whatever `onCall` answers. */
function fakeBus(onCall: (m: Message, reply: (m: Partial<Message>) => void) => void) {
  const dir = mkdtempSync(join(tmpdir(), 'signet-dbus-'))
  const path = join(dir, 'bus')
  const auth: string[] = []
  const sockets: net.Socket[] = []
  let serial = 100
  const server = net.createServer((s) => {
    sockets.push(s)
    let buf = Buffer.alloc(0)
    let state: 'nul' | 'auth' | 'begin' | 'ready' = 'nul'
    const send = (m: Partial<Message>) => s.write(encodeMessage({ type: 2, flags: 1, serial: serial++, body: [], ...m }))
    s.on('data', (d: Buffer) => {
      buf = Buffer.concat([buf, d])
      for (;;) {
        if (state === 'nul') {
          if (!buf.length) return
          expect(buf[0]).toBe(0)
          buf = buf.subarray(1)
          state = 'auth'
        } else if (state === 'auth' || state === 'begin') {
          const i = buf.indexOf('\r\n')
          if (i < 0) return
          auth.push(buf.subarray(0, i).toString())
          buf = buf.subarray(i + 2)
          if (state === 'auth') {
            s.write('OK 0123456789abcdef\r\n')
            state = 'begin'
          } else state = 'ready'
        } else {
          const n = messageLength(buf)
          if (n === null || buf.length < n) return
          const m = decodeMessage(buf.subarray(0, n))
          buf = buf.subarray(n)
          const reply = (r: Partial<Message>) => send({ replySerial: m.serial, ...r })
          if (m.member === 'Hello') reply({ signature: 's', body: [':1.7'] })
          else onCall(m, reply)
        }
      }
    })
  })
  return new Promise<{ path: string; auth: string[]; sockets: net.Socket[]; close: () => void; push: (m: Partial<Message>) => void }>((resolve) =>
    server.listen(path, () =>
      resolve({
        path,
        auth,
        sockets,
        push: (m) => sockets[0]!.write(encodeMessage({ type: 4, flags: 1, serial: serial++, body: [], ...m })),
        close: () => {
          sockets.forEach((s) => s.destroy())
          server.close()
          rmSync(dir, { recursive: true, force: true })
        }
      })
    )
  )
}

let bus: Awaited<ReturnType<typeof fakeBus>> | null = null
afterEach(() => {
  bus?.close()
  bus = null
})

// The fake bus listens on a unix socket path, which Windows refuses (EACCES); the client only runs on Linux anyway.
describe.skipIf(process.platform === 'win32')('DBusClient', () => {
  it('authenticates with EXTERNAL and the hex uid, then says Hello', async () => {
    bus = await fakeBus(() => {})
    const c = await connect(bus.path)
    expect(c.uniqueName).toBe(':1.7')
    expect(bus.auth).toEqual([`AUTH EXTERNAL ${Buffer.from(String(process.getuid!())).toString('hex')}`, 'BEGIN'])
    c.close()
  })

  it('resolves calls with the body and rejects error replies with their name', async () => {
    bus = await fakeBus((m, reply) => {
      if (m.member === 'Ok') reply({ signature: 'a{sv}', body: [new Map([['A', new Variant('u', 5)]])] })
      else reply({ type: 3, errorName: 'org.bluez.Error.InProgress', signature: 's', body: ['busy'] })
    })
    const c = await connect(bus.path)
    const call = (member: string) => c.call({ destination: 'org.bluez', path: '/', interface: 'x.Y', member })
    expect(await call('Ok')).toEqual([new Map([['A', new Variant('u', 5)]])])
    const err = await call('Nope').catch((e) => e)
    expect(err).toBeInstanceOf(DBusError)
    expect(err.dbusName).toBe('org.bluez.Error.InProgress')
    c.close()
  })

  it('delivers signals to handlers until unsubscribed', async () => {
    bus = await fakeBus(() => {})
    const c = await connect(bus.path)
    const got: string[] = []
    const off = c.onSignal((m) => got.push(m.member!))
    const signal = (member: string) => bus!.push({ path: '/p', interface: 'a.B', member })
    signal('One')
    await new Promise((r) => setTimeout(r, 30))
    off()
    signal('Two')
    await new Promise((r) => setTimeout(r, 30))
    expect(got).toEqual(['One'])
    c.close()
  })

  it('sends AddMatch to the bus', async () => {
    const seen: (string | undefined)[] = []
    bus = await fakeBus((m, reply) => {
      seen.push(m.member, m.body[0] as string)
      reply({})
    })
    const c = await connect(bus.path)
    await c.addMatch("type='signal'")
    expect(seen).toEqual(['AddMatch', "type='signal'"])
    c.close()
  })

  it('rejects pending calls when closed, and calls after close', async () => {
    bus = await fakeBus(() => {})
    const c = await connect(bus.path)
    const closed: unknown[] = []
    c.onClose((e) => closed.push(e))
    const p = c.call({ destination: 'x', path: '/', interface: 'a.B', member: 'Never' })
    c.close()
    await expect(p).rejects.toThrow()
    await expect(c.call({ destination: 'x', path: '/', interface: 'a.B', member: 'M' })).rejects.toThrow()
    expect(closed).toHaveLength(1)
    c.close()
    expect(closed).toHaveLength(1)
  })

  it('rejects pending calls and reports when the bus goes away', async () => {
    bus = await fakeBus(() => {})
    const c = await connect(bus.path)
    const closed = new Promise((r) => c.onClose(r))
    const p = c.call({ destination: 'x', path: '/', interface: 'a.B', member: 'Never' })
    bus.sockets[0]!.destroy()
    await expect(p).rejects.toThrow()
    await closed
    expect(c.isClosed).toBe(true)
  })

  it('closes on garbage from the bus instead of crashing', async () => {
    bus = await fakeBus(() => {})
    const c = await connect(bus.path)
    const closed = new Promise<Error | undefined>((r) => c.onClose(r))
    bus.sockets[0]!.write(Buffer.alloc(32, 0x55))
    expect(await closed).toBeInstanceOf(Error)
  })

  it('fails to connect to a missing socket or when auth is refused', async () => {
    await expect(connect('/nonexistent/signet-bus')).rejects.toThrow()
    const dir = mkdtempSync(join(tmpdir(), 'signet-dbus-'))
    const path = join(dir, 'bus')
    const server = net.createServer((s) => s.on('data', () => s.write('REJECTED EXTERNAL\r\n')))
    await new Promise<void>((r) => server.listen(path, r))
    await expect(connect(path)).rejects.toThrow(/refused/)
    server.close()
    rmSync(dir, { recursive: true, force: true })
  })
})

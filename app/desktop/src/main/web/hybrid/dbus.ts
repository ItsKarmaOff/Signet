import net from 'node:net'
import { DBusWireError, Flags, MessageType, decodeMessage, encodeMessage, messageLength, type Message } from './dbus-wire'

// Minimal D-Bus client over a unix socket (system bus): SASL EXTERNAL, Hello, method calls, signals and match rules. No
// dbus library on purpose. The wire format lives in dbus-wire.ts. Only for Linux (BlueZ scan), no Electron import.

const DEFAULT_SYSTEM_BUS = '/var/run/dbus/system_bus_socket'
const AUTH_TIMEOUT = 10_000
const CALL_TIMEOUT = 15_000
const BUS = 'org.freedesktop.DBus'

/** An error reply: `dbusName` is the D-Bus error name (e.g. org.bluez.Error.InProgress). */
export class DBusError extends Error {
  constructor(
    readonly dbusName: string,
    message: string
  ) {
    super(message ? `${dbusName}: ${message}` : dbusName)
  }
}

export interface Call {
  destination: string
  path: string
  interface: string
  member: string
  signature?: string
  body?: unknown[]
}

/** Socket path of a bus address (`unix:path=...` or `unix:abstract=...`, first usable entry), null for other transports. */
export function parseBusAddress(address: string): string | null {
  for (const entry of address.split(';')) {
    if (!entry.startsWith('unix:')) continue
    const params = new Map<string, string>()
    for (const kv of entry.slice(5).split(',')) {
      const i = kv.indexOf('=')
      if (i > 0) params.set(kv.slice(0, i), unescapeValue(kv.slice(i + 1)))
    }
    const path = params.get('path')
    if (path) return path
    const abstract = params.get('abstract')
    // A leading NUL selects the abstract namespace.
    if (abstract) return `\0${abstract}`
  }
  return null
}

function unescapeValue(v: string): string {
  return Buffer.from(
    v.replace(/%([0-9a-fA-F]{2})/g, (_m, h: string) => String.fromCharCode(parseInt(h, 16))),
    'latin1'
  ).toString('utf8')
}

/** Where the system bus is: DBUS_SYSTEM_BUS_ADDRESS, else the default socket. */
export function systemBusPath(env: NodeJS.ProcessEnv = process.env): string | null {
  const a = env.DBUS_SYSTEM_BUS_ADDRESS
  return a ? parseBusAddress(a) : DEFAULT_SYSTEM_BUS
}

interface Pending {
  resolve: (body: unknown[]) => void
  reject: (e: Error) => void
  timer: ReturnType<typeof setTimeout>
}

export class DBusClient {
  private serial = 0
  private pending = new Map<number, Pending>()
  private signalHandlers = new Set<(m: Message) => void>()
  private closeHandlers = new Set<(e?: Error) => void>()
  private buf: Buffer = Buffer.alloc(0)
  private closed = false
  /** Unique name given by the bus (":1.42"). */
  uniqueName = ''

  /** Use `connect`. `rest` is data that came after the auth reply. */
  constructor(
    private socket: net.Socket,
    rest: Buffer
  ) {
    socket.on('data', (d: Buffer) => this.feed(d))
    socket.on('error', (e) => this.fail(e))
    socket.on('close', () => this.fail(new Error('D-Bus connection closed')))
    if (rest.length) this.feed(rest)
  }

  get isClosed(): boolean {
    return this.closed
  }

  private feed(chunk: Buffer): void {
    this.buf = this.buf.length ? Buffer.concat([this.buf, chunk]) : chunk
    try {
      for (;;) {
        const total = messageLength(this.buf)
        if (total === null || this.buf.length < total) return
        const message = decodeMessage(this.buf.subarray(0, total))
        this.buf = this.buf.subarray(total)
        this.dispatch(message)
      }
    } catch (e) {
      // Malformed stream: the connection cannot be trusted anymore.
      this.fail(e instanceof Error ? e : new DBusWireError('bad data'))
    }
  }

  private dispatch(m: Message): void {
    if (m.type === MessageType.MethodReturn || m.type === MessageType.Error) {
      const p = m.replySerial !== undefined ? this.pending.get(m.replySerial) : undefined
      if (!p) return
      this.pending.delete(m.replySerial!)
      clearTimeout(p.timer)
      if (m.type === MessageType.Error) p.reject(new DBusError(m.errorName ?? 'unknown', typeof m.body[0] === 'string' ? m.body[0] : ''))
      else p.resolve(m.body)
    } else if (m.type === MessageType.Signal) {
      for (const h of [...this.signalHandlers]) {
        try {
          h(m)
        } catch (e) {
          console.error('[dbus] signal handler failed:', e)
        }
      }
    } else if (m.type === MessageType.MethodCall && !(m.flags & Flags.NoReplyExpected)) {
      // Nothing is exported by this client.
      this.send({
        type: MessageType.Error,
        flags: Flags.NoReplyExpected,
        serial: this.nextSerial(),
        errorName: 'org.freedesktop.DBus.Error.UnknownMethod',
        replySerial: m.serial,
        destination: m.sender,
        signature: 's',
        body: ['Not implemented']
      })
    }
  }

  private nextSerial(): number {
    this.serial = this.serial >= 0xffffffff ? 1 : this.serial + 1
    return this.serial
  }

  private send(m: Message): void {
    if (this.closed) throw new Error('D-Bus connection closed')
    this.socket.write(encodeMessage(m))
  }

  /** Calls a method; resolves with the reply body, rejects with a `DBusError` for an error reply. */
  call(c: Call): Promise<unknown[]> {
    return new Promise((resolve, reject) => {
      try {
        const serial = this.nextSerial()
        const timer = setTimeout(() => {
          this.pending.delete(serial)
          reject(new Error(`D-Bus call timed out: ${c.interface}.${c.member}`))
        }, CALL_TIMEOUT)
        this.pending.set(serial, { resolve, reject, timer })
        try {
          this.send({
            type: MessageType.MethodCall,
            flags: 0,
            serial,
            destination: c.destination,
            path: c.path,
            interface: c.interface,
            member: c.member,
            signature: c.signature,
            body: c.body ?? []
          })
        } catch (e) {
          clearTimeout(timer)
          this.pending.delete(serial)
          throw e
        }
      } catch (e) {
        reject(e instanceof Error ? e : new Error(String(e)))
      }
    })
  }

  /** Subscribes to every signal received (filter with `addMatch`); returns the unsubscribe function. */
  onSignal(handler: (m: Message) => void): () => void {
    this.signalHandlers.add(handler)
    return () => this.signalHandlers.delete(handler)
  }

  /** Asks the bus to route the signals matching `rule` to this client. */
  async addMatch(rule: string): Promise<void> {
    await this.call({ destination: BUS, path: '/org/freedesktop/DBus', interface: BUS, member: 'AddMatch', signature: 's', body: [rule] })
  }

  /** Called once when the connection ends (closed by us or not). */
  onClose(handler: (e?: Error) => void): () => void {
    this.closeHandlers.add(handler)
    return () => this.closeHandlers.delete(handler)
  }

  close(): void {
    this.fail()
  }

  private fail(e?: Error): void {
    if (this.closed) return
    this.closed = true
    this.socket.destroy()
    const err = e ?? new Error('D-Bus connection closed')
    for (const p of this.pending.values()) {
      clearTimeout(p.timer)
      p.reject(err)
    }
    this.pending.clear()
    this.signalHandlers.clear()
    for (const h of [...this.closeHandlers]) {
      try {
        h(e)
      } catch {
        // closing must go on
      }
    }
    this.closeHandlers.clear()
  }
}

/** Opens the system bus: socket, SASL EXTERNAL authentication, then `Hello`. */
export async function connect(path: string | null = systemBusPath()): Promise<DBusClient> {
  if (!path) throw new Error('No usable D-Bus address (only unix sockets are supported)')
  const uid = process.getuid?.()
  if (uid === undefined) throw new Error('D-Bus needs a unix user id')
  const { socket, rest } = await authenticate(path, uid)
  const client = new DBusClient(socket, rest)
  try {
    const [name] = await client.call({ destination: BUS, path: '/org/freedesktop/DBus', interface: BUS, member: 'Hello' })
    if (typeof name !== 'string') throw new Error('bad reply to Hello')
    client.uniqueName = name
  } catch (e) {
    client.close()
    throw e
  }
  return client
}

function authenticate(path: string, uid: number): Promise<{ socket: net.Socket; rest: Buffer }> {
  return new Promise((resolve, reject) => {
    const socket = net.createConnection({ path })
    let data = Buffer.alloc(0)
    let sentBegin = false
    const timer = setTimeout(() => fail(new Error('D-Bus authentication timed out')), AUTH_TIMEOUT)
    const fail = (e: Error): void => {
      clearTimeout(timer)
      socket.destroy()
      reject(e)
    }
    const onData = (d: Buffer): void => {
      data = Buffer.concat([data, d])
      const end = data.indexOf('\r\n')
      if (end < 0) {
        if (data.length > 4096) fail(new Error('D-Bus authentication reply too long'))
        return
      }
      const line = data.subarray(0, end).toString('latin1')
      if (!line.startsWith('OK') || sentBegin) return fail(new Error('D-Bus authentication refused'))
      sentBegin = true
      clearTimeout(timer)
      socket.removeListener('data', onData)
      socket.removeListener('error', fail)
      socket.removeListener('close', onClose)
      socket.write('BEGIN\r\n')
      resolve({ socket, rest: data.subarray(end + 2) })
    }
    socket.on('data', onData)
    socket.once('error', fail)
    const onClose = (): void => fail(new Error('D-Bus connection closed during authentication'))
    socket.once('close', onClose)
    socket.once('connect', () => {
      // A NUL byte, then the user id as ASCII decimal digits written in hex.
      socket.write(Buffer.concat([Buffer.from([0]), Buffer.from(`AUTH EXTERNAL ${Buffer.from(String(uid), 'ascii').toString('hex')}\r\n`)]))
    })
  })
}

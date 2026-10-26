import { describe, expect, it } from 'vitest'
import { DBusWireError, MAX_MESSAGE, MessageType, Variant, decodeMessage, encodeMessage, marshal, messageLength, unmarshal } from './dbus-wire'

const roundTrip = (sig: string, values: unknown[], le = true) => unmarshal(sig, marshal(sig, values, le), le)

describe('basic types', () => {
  it.each([true, false])('round-trips every basic type (little-endian: %s)', (le) => {
    const values = [200, true, -1234, 65000, -70000, 4000000000, -(2n ** 62n), 2n ** 63n, 1.5, 'héllo', '/org/bluez', 'a{sv}']
    expect(roundTrip('ybnqiuxtdsog', values, le)).toEqual(values)
  })

  it('accepts safe integers for 64-bit types and rejects out of range values', () => {
    expect(roundTrip('xt', [5, 6])).toEqual([5n, 6n])
    expect(() => marshal('y', [256])).toThrow(DBusWireError)
    expect(() => marshal('u', [-1])).toThrow(DBusWireError)
    expect(() => marshal('t', [-1n])).toThrow(DBusWireError)
    expect(() => marshal('s', ['a\0b'])).toThrow(DBusWireError)
    expect(() => marshal('b', [1])).toThrow(DBusWireError)
    expect(() => marshal('s', [])).toThrow(DBusWireError)
  })

  it('pads to the alignment of each type', () => {
    expect([...marshal('yu', [1, 2])]).toEqual([1, 0, 0, 0, 2, 0, 0, 0])
    expect(marshal('yx', [1, 2]).length).toBe(16)
    expect(marshal('yn', [1, 2]).length).toBe(4)
    expect(marshal('ys', [1, 'a']).length).toBe(4 + 4 + 2)
    expect(marshal('yg', [1, 'a']).length).toBe(1 + 1 + 1 + 1)
    expect(marshal('y(yy)', [1, [2, 3]]).length).toBe(10)
    // An empty array of 8-aligned elements still pads to 8.
    expect(marshal('ax', [[]]).length).toBe(8)
    expect(marshal('yax', [1, []]).length).toBe(8)
  })
})

describe('containers', () => {
  it('round-trips arrays, byte arrays, structs and variants', () => {
    expect(roundTrip('as', [['a', 'bc', '']])).toEqual([['a', 'bc', '']])
    expect(roundTrip('ay', [Buffer.from([1, 2, 3])])).toEqual([Buffer.from([1, 2, 3])])
    expect(roundTrip('ay', [new Uint8Array(0)])).toEqual([Buffer.alloc(0)])
    expect(roundTrip('(ysx)', [[1, 'x', 9n]])).toEqual([[1, 'x', 9n]])
    expect(roundTrip('aai', [[[1, 2], [], [3]]])).toEqual([[[1, 2], [], [3]]])
    expect(roundTrip('v', [new Variant('s', 'hi')])).toEqual([new Variant('s', 'hi')])
    expect(roundTrip('v', [new Variant('v', new Variant('u', 7))])).toEqual([new Variant('v', new Variant('u', 7))])
  })

  it('a{sv} with a variant holding ay', () => {
    const dict = new Map<string, Variant>([
      ['Name', new Variant('s', 'phone')],
      ['0000fff9-0000-1000-8000-00805f9b34fb', new Variant('ay', Buffer.alloc(20, 7))],
      ['Powered', new Variant('b', true)]
    ])
    const [out] = roundTrip('a{sv}', [dict]) as [Map<string, Variant>]
    expect(out.get('Name')).toEqual(new Variant('s', 'phone'))
    expect((out.get('0000fff9-0000-1000-8000-00805f9b34fb')!.value as Buffer).equals(Buffer.alloc(20, 7))).toBe(true)
    expect(out.get('Powered')!.value).toBe(true)
  })

  it('a{oa{sa{sv}}} as GetManagedObjects returns it', () => {
    const props = new Map([['Powered', new Variant('b', false)]])
    const value = new Map([['/org/bluez/hci0', new Map([['org.bluez.Adapter1', props]])]])
    const sig = 'a{oa{sa{sv}}}'
    expect(roundTrip(sig, [value], true)).toEqual([value])
    expect(roundTrip(sig, [value], false)).toEqual([value])
    // Dicts may also be given as arrays of pairs.
    expect(unmarshal('a{ss}', marshal('a{ss}', [[['k', 'v']]]))).toEqual([new Map([['k', 'v']])])
  })

  it('marshals the byte-for-byte layout of a dict entry', () => {
    // a{yu}: length 8, entry aligned to 8 (after the 4 length bytes), key, pad, value.
    expect([...marshal('a{yu}', [new Map([[1, 2]])])]).toEqual([8, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 2, 0, 0, 0])
  })
})

describe('malformed input', () => {
  it('rejects bad signatures', () => {
    for (const sig of ['a', '(', '()', '(i', '{ss}', 'a{(i)s}', 'a{ss', 'z', 'a{s}', 'a{sss}']) {
      expect(() => unmarshal(sig, Buffer.alloc(64))).toThrow(DBusWireError)
    }
    expect(() => marshal('a'.repeat(40) + 'i', [])).toThrow(DBusWireError)
  })

  it('rejects truncated, trailing, mis-padded and mis-terminated data', () => {
    const s = marshal('s', ['hello'])
    expect(() => unmarshal('s', s.subarray(0, s.length - 1))).toThrow(DBusWireError)
    expect(() => unmarshal('s', Buffer.concat([s, Buffer.alloc(1)]))).toThrow(DBusWireError)
    const bad = Buffer.from(s)
    bad[bad.length - 1] = 1
    expect(() => unmarshal('s', bad)).toThrow(DBusWireError)
    expect(() => unmarshal('yu', Buffer.from([1, 9, 0, 0, 2, 0, 0, 0]))).toThrow(DBusWireError)
    expect(() => unmarshal('b', Buffer.from([2, 0, 0, 0]))).toThrow(DBusWireError)
    expect(() => unmarshal('i', Buffer.alloc(0))).toThrow(DBusWireError)
  })

  it('rejects an array longer than the data or not made of whole elements', () => {
    expect(() => unmarshal('ay', Buffer.from([255, 255, 255, 127, 1]))).toThrow(DBusWireError)
    expect(() => unmarshal('au', Buffer.from([6, 0, 0, 0, 1, 0, 0, 0, 2, 0, 0, 0]))).toThrow(DBusWireError)
    expect(() => unmarshal('ai', Buffer.from([0xff, 0xff, 0xff, 0xff]))).toThrow(DBusWireError)
  })

  it('rejects absurdly nested variants', () => {
    let v = new Variant('u', 1)
    for (let i = 0; i < 40; i++) v = new Variant('v', v)
    expect(() => marshal('v', [v])).toThrow(DBusWireError)
    const bytes = Buffer.concat([...Array(40).fill(Buffer.from([1, 0x76, 0])), Buffer.from([1, 0x75, 0, 0, 1, 0, 0, 0])])
    expect(() => unmarshal('v', bytes)).toThrow(DBusWireError)
  })
})

describe('messages', () => {
  it('round-trips a method call, with the fields in the right places', () => {
    const m = {
      type: MessageType.MethodCall,
      flags: 0,
      serial: 3,
      destination: 'org.bluez',
      path: '/org/bluez/hci0',
      interface: 'org.bluez.Adapter1',
      member: 'SetDiscoveryFilter',
      signature: 'a{sv}',
      body: [new Map([['Transport', new Variant('s', 'le')]])]
    }
    const bytes = encodeMessage(m)
    expect(bytes[0]).toBe(0x6c)
    expect(bytes[1]).toBe(1)
    expect(bytes[3]).toBe(1)
    expect(bytes.readUInt32LE(8)).toBe(3)
    expect(messageLength(bytes)).toBe(bytes.length)
    expect(decodeMessage(bytes)).toEqual(m)
  })

  it('encodes a body-less call without a signature field', () => {
    const bytes = encodeMessage({
      type: 1,
      flags: 0,
      serial: 1,
      path: '/',
      member: 'Hello',
      destination: 'org.freedesktop.DBus',
      interface: 'org.freedesktop.DBus',
      body: []
    })
    expect(bytes.readUInt32LE(4)).toBe(0)
    expect(bytes.length % 8).toBe(0)
    expect(decodeMessage(bytes).signature).toBeUndefined()
  })

  it('parses a recorded-looking reply to Hello', () => {
    const f = (...parts: (number[] | string)[]) => Buffer.concat(parts.map((p) => (typeof p === 'string' ? Buffer.from(p, 'latin1') : Buffer.from(p))))
    const reply = f(
      [0x6c, 2, 1, 1, 10, 0, 0, 0, 1, 0, 0, 0, 63, 0, 0, 0],
      // REPLY_SERIAL u
      [5, 1, 0x75, 0, 1, 0, 0, 0],
      // DESTINATION s
      [6, 1, 0x73, 0, 5, 0, 0, 0],
      ':1.42\0',
      [0, 0],
      // SENDER s
      [7, 1, 0x73, 0, 20, 0, 0, 0],
      'org.freedesktop.DBus\0',
      [0, 0, 0],
      // SIGNATURE g
      [8, 1, 0x67, 0, 1, 0x73, 0],
      // pad to 8, then the body
      [0],
      [5, 0, 0, 0],
      ':1.42\0'
    )
    expect(reply.length).toBe(90)
    expect(messageLength(reply)).toBe(90)
    expect(decodeMessage(reply)).toEqual({
      type: MessageType.MethodReturn,
      flags: 1,
      serial: 1,
      replySerial: 1,
      destination: ':1.42',
      sender: 'org.freedesktop.DBus',
      signature: 's',
      body: [':1.42']
    })
  })

  it('parses a big-endian message', () => {
    const reply = Buffer.from([0x42, 2, 0, 1, 0, 0, 0, 0, 0, 0, 0, 9, 0, 0, 0, 8, 5, 1, 0x75, 0, 0, 0, 0, 4])
    expect(decodeMessage(reply)).toMatchObject({ type: 2, serial: 9, replySerial: 4, body: [] })
  })

  it('round-trips an error and a signal', () => {
    const err = {
      type: 3,
      flags: 1,
      serial: 5,
      errorName: 'org.bluez.Error.InProgress',
      replySerial: 2,
      signature: 's',
      body: ['Operation already in progress']
    }
    expect(decodeMessage(encodeMessage(err))).toEqual(err)
    const sig = {
      type: 4,
      flags: 1,
      serial: 6,
      path: '/org/bluez/hci0',
      interface: 'org.freedesktop.DBus.Properties',
      member: 'PropertiesChanged',
      sender: ':1.3',
      signature: 'sa{sv}as',
      body: ['org.bluez.Adapter1', new Map([['Powered', new Variant('b', true)]]), []]
    }
    expect(decodeMessage(encodeMessage(sig))).toEqual(sig)
  })

  it('rejects malformed messages without throwing anything else', () => {
    const good = encodeMessage({ type: 1, flags: 0, serial: 1, path: '/', member: 'M', signature: 's', body: ['x'] })
    expect(() => decodeMessage(good.subarray(0, good.length - 1))).toThrow(DBusWireError)
    expect(() => decodeMessage(Buffer.concat([good, Buffer.alloc(1)]))).toThrow(DBusWireError)
    const patch = (at: number, v: number) => {
      const b = Buffer.from(good)
      b[at] = v
      return b
    }
    expect(() => decodeMessage(patch(0, 0x00))).toThrow(DBusWireError)
    expect(() => decodeMessage(patch(1, 9))).toThrow(DBusWireError)
    expect(() => decodeMessage(patch(3, 2))).toThrow(DBusWireError)
    expect(() => decodeMessage(patch(8, 0))).toThrow(DBusWireError)
    const big = Buffer.from(good)
    big.writeUInt32LE(MAX_MESSAGE, 4)
    expect(() => messageLength(big)).toThrow(DBusWireError)
    expect(messageLength(good.subarray(0, 15))).toBeNull()
    // Method call without a member.
    expect(() => decodeMessage(encodeMessage({ type: 1, flags: 0, serial: 1, path: '/', body: [] }))).toThrow(DBusWireError)
    // Random garbage never throws anything but DBusWireError.
    for (let i = 0; i < 200; i++) {
      const g = Buffer.from(good)
      for (let k = 0; k < 3; k++) g[Math.floor(Math.random() * g.length)] = Math.floor(Math.random() * 256)
      try {
        decodeMessage(g)
      } catch (e) {
        expect(e).toBeInstanceOf(DBusWireError)
      }
    }
  })
})

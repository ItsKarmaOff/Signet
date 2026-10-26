import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { startBleScan, type BleState } from './bluez'
import { DBusError } from './dbus'
import { MessageType, Variant, type Message } from './dbus-wire'

const FIDO = '0000fff9-0000-1000-8000-00805f9b34fb'
const GOOGLE = '0000fde2-0000-1000-8000-00805f9b34fb'
const OTHER = '0000feaa-0000-1000-8000-00805f9b34fb'
const HCI0 = '/org/bluez/hci0'

const adapter = (powered: boolean): Map<string, Map<string, Variant>> => new Map([['org.bluez.Adapter1', new Map([['Powered', new Variant('b', powered)]])]])
const device = (data: Record<string, number[]>): Map<string, Map<string, Variant>> =>
  new Map([
    [
      'org.bluez.Device1',
      new Map([['ServiceData', new Variant('a{sv}', new Map(Object.entries(data).map(([k, v]) => [k, new Variant('ay', Buffer.from(v))])))]])
    ]
  ])

type Call = { member: string; path: string; body: unknown[] }

function setup(objects: Map<string, Map<string, Map<string, Variant>>>, failOn: Record<string, Error> = {}) {
  const calls: Call[] = []
  const matches: string[] = []
  let handler: (m: Message) => void = () => {}
  let onClose: (e?: Error) => void = () => {}
  let closed = 0
  const client = {
    call: vi.fn(async (c: { member: string; path: string; body?: unknown[] }) => {
      calls.push({ member: c.member, path: c.path, body: c.body ?? [] })
      if (failOn[c.member]) throw failOn[c.member]
      return c.member === 'GetManagedObjects' ? [objects] : []
    }),
    addMatch: vi.fn(async (r: string) => void matches.push(r)),
    onSignal: (h: (m: Message) => void) => {
      handler = h
      return () => {}
    },
    onClose: (h: (e?: Error) => void) => {
      onClose = h
      return () => {}
    },
    close: () => void closed++
  }
  const states: BleState[] = []
  const data: [string, number[]][] = []
  const stop = startBleScan({
    uuids: [FIDO, GOOGLE],
    onServiceData: (u, b) => data.push([u, [...b]]),
    onState: (s) => states.push(s),
    connect: async () => client
  })
  const signal = (m: Partial<Message>) => handler({ type: MessageType.Signal, flags: 1, serial: 1, body: [], ...m })
  return { calls, matches, states, data, stop, signal, closed: () => closed, drop: (e?: Error) => onClose(e) }
}

const flush = () => vi.advanceTimersByTimeAsync(0)

beforeEach(() => vi.useFakeTimers())
afterEach(() => vi.useRealTimers())

describe('startBleScan', () => {
  it('reports no adapter', async () => {
    const s = setup(new Map())
    await flush()
    expect(s.states).toEqual(['no-adapter'])
    expect(s.matches).toHaveLength(3)
    expect(s.matches[0]).toContain("path_namespace='/org/bluez'")
    expect(s.matches[1]).toContain('InterfacesAdded')
  })

  it('filters and starts discovery on a powered adapter, preferring it', async () => {
    const s = setup(
      new Map([
        ['/org/bluez/hci1', adapter(false)],
        [HCI0, adapter(true)]
      ])
    )
    await flush()
    expect(s.states).toEqual(['scanning'])
    expect(s.calls.map((c) => [c.member, c.path])).toEqual([
      ['GetManagedObjects', '/'],
      ['SetDiscoveryFilter', HCI0],
      ['StartDiscovery', HCI0]
    ])
    const filter = s.calls[1]!.body[0] as Map<string, Variant>
    // No UUID filter in BlueZ (service data is filtered by Signet).
    expect(filter.has('UUIDs')).toBe(false)
    expect(filter.get('Transport')).toEqual(new Variant('s', 'le'))
    expect(filter.get('DuplicateData')).toEqual(new Variant('b', true))
  })

  it('waits when Bluetooth is off, and starts when it is turned on (never powers it itself)', async () => {
    const s = setup(new Map([[HCI0, adapter(false)]]))
    await flush()
    expect(s.states).toEqual(['off'])
    expect(s.calls.map((c) => c.member)).toEqual(['GetManagedObjects'])
    s.signal({
      path: HCI0,
      interface: 'org.freedesktop.DBus.Properties',
      member: 'PropertiesChanged',
      body: ['org.bluez.Adapter1', new Map([['Powered', new Variant('b', true)]]), []]
    })
    await flush()
    expect(s.states).toEqual(['off', 'scanning'])
    expect(s.calls.map((c) => c.member)).toEqual(['GetManagedObjects', 'SetDiscoveryFilter', 'StartDiscovery'])
    // Turned off again, then on again.
    s.signal({
      path: HCI0,
      interface: 'org.freedesktop.DBus.Properties',
      member: 'PropertiesChanged',
      body: ['org.bluez.Adapter1', new Map([['Powered', new Variant('b', false)]]), []]
    })
    await flush()
    expect(s.states).toEqual(['off', 'scanning', 'off'])
  })

  it('picks up an adapter that appears later', async () => {
    const s = setup(new Map())
    await flush()
    s.signal({ path: '/', interface: 'org.freedesktop.DBus.ObjectManager', member: 'InterfacesAdded', body: [HCI0, adapter(true)] })
    await flush()
    expect(s.states).toEqual(['no-adapter', 'scanning'])
  })

  it('reports service data of known devices, new devices and updates, for our UUIDs only', async () => {
    const s = setup(
      new Map([
        [HCI0, adapter(true)],
        [`${HCI0}/dev_A`, device({ [FIDO.toUpperCase()]: [1, 2], [OTHER]: [9] })]
      ])
    )
    await flush()
    expect(s.data).toEqual([[FIDO, [1, 2]]])
    s.signal({ path: '/', interface: 'org.freedesktop.DBus.ObjectManager', member: 'InterfacesAdded', body: [`${HCI0}/dev_B`, device({ [GOOGLE]: [3] })] })
    s.signal({
      path: `${HCI0}/dev_A`,
      interface: 'org.freedesktop.DBus.Properties',
      member: 'PropertiesChanged',
      body: ['org.bluez.Device1', device({ [FIDO]: [4] }).get('org.bluez.Device1'), []]
    })
    s.signal({
      path: `${HCI0}/dev_A`,
      interface: 'org.freedesktop.DBus.Properties',
      member: 'PropertiesChanged',
      body: ['org.bluez.Device1', new Map([['RSSI', new Variant('n', -50)]]), []]
    })
    expect(s.data).toEqual([
      [FIDO, [1, 2]],
      [GOOGLE, [3]],
      [FIDO, [4]]
    ])
  })

  it('ignores malformed signals', async () => {
    const s = setup(new Map([[HCI0, adapter(true)]]))
    await flush()
    const spy = vi.spyOn(console, 'error').mockImplementation(() => {})
    s.signal({ path: HCI0, interface: 'org.freedesktop.DBus.Properties', member: 'PropertiesChanged', body: [] })
    s.signal({ path: HCI0, interface: 'org.freedesktop.DBus.Properties', member: 'PropertiesChanged', body: ['org.bluez.Device1', 5, 7] })
    s.signal({ path: '/', interface: 'org.freedesktop.DBus.ObjectManager', member: 'InterfacesAdded', body: [HCI0, 'x'] })
    expect(s.data).toEqual([])
    spy.mockRestore()
  })

  it('treats InProgress as fine', async () => {
    const s = setup(new Map([[HCI0, adapter(true)]]), { StartDiscovery: new DBusError('org.bluez.Error.InProgress', 'busy') })
    await flush()
    expect(s.states).toEqual(['scanning'])
  })

  it('reports denied and error', async () => {
    const spy = vi.spyOn(console, 'error').mockImplementation(() => {})
    const a = setup(new Map([[HCI0, adapter(true)]]), { StartDiscovery: new DBusError('org.freedesktop.DBus.Error.AccessDenied', 'no') })
    await flush()
    expect(a.states).toEqual(['denied'])
    const b = setup(new Map([[HCI0, adapter(true)]]), { SetDiscoveryFilter: new DBusError('org.bluez.Error.Failed', 'x') })
    await flush()
    expect(b.states).toEqual(['error'])
    const c = setup(new Map([[HCI0, adapter(true)]]), { GetManagedObjects: new DBusError('org.freedesktop.DBus.Error.ServiceUnknown', 'x') })
    await flush()
    expect(c.states).toEqual(['no-adapter'])
    spy.mockRestore()
  })

  it('reports denied when the system bus is unreachable', async () => {
    const spy = vi.spyOn(console, 'error').mockImplementation(() => {})
    const states: BleState[] = []
    startBleScan({ uuids: [FIDO], onServiceData: () => {}, onState: (s) => states.push(s), connect: () => Promise.reject(new Error('ENOENT')) })
    await flush()
    expect(states).toEqual(['denied'])
    spy.mockRestore()
  })

  it('reports an error when the bus connection is lost during the scan', async () => {
    const spy = vi.spyOn(console, 'error').mockImplementation(() => {})
    const s = setup(new Map([[HCI0, adapter(true)]]))
    await flush()
    s.drop(new Error('gone'))
    expect(s.states).toEqual(['scanning', 'error'])
    spy.mockRestore()
  })

  it('stops discovery then closes, once, and says nothing afterwards', async () => {
    const s = setup(new Map([[HCI0, adapter(true)]]))
    await flush()
    s.stop()
    s.stop()
    await flush()
    expect(s.calls.filter((c) => c.member === 'StopDiscovery')).toHaveLength(1)
    expect(s.closed()).toBe(1)
    s.drop()
    s.signal({
      path: HCI0,
      interface: 'org.freedesktop.DBus.Properties',
      member: 'PropertiesChanged',
      body: ['org.bluez.Adapter1', new Map([['Powered', new Variant('b', false)]]), []]
    })
    expect(s.states).toEqual(['scanning'])
  })

  it('closes the connection if stopped before it is ready', async () => {
    const s = setup(new Map([[HCI0, adapter(true)]]))
    s.stop()
    await flush()
    expect(s.closed()).toBe(1)
    expect(s.states).toEqual([])
    expect(s.calls).toEqual([])
  })

  it('closes without waiting for StopDiscovery forever', async () => {
    const s = setup(new Map([[HCI0, adapter(true)]]))
    await flush()
    s.stop()
    expect(s.closed()).toBe(0)
    await flush()
    expect(s.closed()).toBe(1)
  })
})

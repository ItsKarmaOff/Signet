import { connect, DBusError, type DBusClient } from './dbus'
import { Variant, type Message } from './dbus-wire'

// BLE scan through BlueZ over D-Bus (system bus, no root), to catch the phone's advert of the "hybrid" transport. Linux
// only (loaded on demand). No Electron import. The connection stays open for the whole scan: BlueZ stops the discovery
// of a client that disconnects.

export type BleState = 'scanning' | 'off' | 'no-adapter' | 'denied' | 'error'

export interface BleScanOptions {
  /** 128-bit service UUIDs to scan for (lowercase). */
  uuids: readonly string[]
  /** An advert (or a cached device) carries service data for one of the UUIDs. */
  onServiceData: (uuid: string, bytes: Buffer) => void
  onState: (state: BleState) => void
  /** Bus connection, replaced in tests. */
  connect?: () => Promise<Pick<DBusClient, 'call' | 'addMatch' | 'onSignal' | 'onClose' | 'close'>>
}

const BLUEZ = 'org.bluez'
const ADAPTER = 'org.bluez.Adapter1'
const DEVICE = 'org.bluez.Device1'
const PROPS = 'org.freedesktop.DBus.Properties'
const OBJECTS = 'org.freedesktop.DBus.ObjectManager'
/** Wait for StopDiscovery at most this long before closing. */
const STOP_WAIT = 1000

type Props = Map<string, Variant>
type Interfaces = Map<string, Props>

const asProps = (v: unknown): Props => (v instanceof Map ? (v as Props) : new Map())

/** Starts scanning; returns the stop function (idempotent). Failures are reported through `onState`. */
export function startBleScan(options: BleScanOptions): () => void {
  const wanted = new Set(options.uuids.map((u) => u.toLowerCase()))
  let stopped = false
  let client: Awaited<ReturnType<NonNullable<BleScanOptions['connect']>>> | null = null
  let last: BleState | null = null
  /** Adapters by object path: powered or not. */
  const adapters = new Map<string, boolean>()
  /** Adapter currently scanning, and the one a start is in progress for. */
  let scanning: string | null = null
  let starting: string | null = null

  const state = (s: BleState): void => {
    if (stopped || s === last) return
    last = s
    options.onState(s)
  }

  const serviceData = (props: Props): void => {
    const sd = props.get('ServiceData')?.value
    if (!(sd instanceof Map)) return
    for (const [key, v] of sd as Map<unknown, unknown>) {
      const uuid = typeof key === 'string' ? key.toLowerCase() : ''
      if (wanted.has(uuid) && v instanceof Variant && Buffer.isBuffer(v.value)) options.onServiceData(uuid, v.value)
    }
  }

  const fail = (e: unknown, what: string): void => {
    const name = e instanceof DBusError ? e.dbusName : ''
    console.error(`Bluetooth: ${what}:`, e instanceof Error ? e.message : e)
    state(/AccessDenied|NotAuthorized|NotPermitted/.test(name) ? 'denied' : 'error')
  }

  /** Picks the adapter to scan on (a powered one first) and starts or stops accordingly. */
  const evaluate = (): void => {
    if (stopped || !client) return
    if (scanning && adapters.get(scanning) !== true) scanning = null
    if (scanning) return
    if (!adapters.size) return state('no-adapter')
    const powered = [...adapters].find(([, on]) => on)
    if (!powered) return state('off')
    if (starting) return
    void startDiscovery(powered[0])
  }

  const startDiscovery = async (path: string): Promise<void> => {
    starting = path
    try {
      const c = client!
      // No UUIDs in the filter: depending on the BlueZ version it only matches the advertised UUID list, not the service
      // data the phone sends. Adverts are filtered here instead (`serviceData`).
      const filter = new Map<string, Variant>([
        ['Transport', new Variant('s', 'le')],
        ['DuplicateData', new Variant('b', true)]
      ])
      await c.call({ destination: BLUEZ, path, interface: ADAPTER, member: 'SetDiscoveryFilter', signature: 'a{sv}', body: [filter] })
      try {
        await c.call({ destination: BLUEZ, path, interface: ADAPTER, member: 'StartDiscovery' })
      } catch (e) {
        // Already discovering (for us or another client): fine.
        if (!(e instanceof DBusError && e.dbusName === 'org.bluez.Error.InProgress')) throw e
      }
      starting = null
      if (stopped) return
      scanning = path
      // The adapter may have been turned off or removed meanwhile.
      if (adapters.get(path) === true) state('scanning')
      else evaluate()
    } catch (e) {
      starting = null
      // No retry: a failed start is final for this scan.
      if (!stopped) fail(e, 'could not start the scan')
    }
  }

  const addAdapter = (ifaces: Interfaces, path: string): void => {
    const a = ifaces.get(ADAPTER)
    if (a) adapters.set(path, a.get('Powered')?.value === true)
  }

  const onSignal = (m: Message): void => {
    if (stopped) return
    try {
      if (m.interface === OBJECTS && m.member === 'InterfacesAdded' && typeof m.body[0] === 'string') {
        const ifaces = m.body[1] as Interfaces
        addAdapter(ifaces, m.body[0])
        const dev = ifaces.get(DEVICE)
        if (dev) serviceData(dev)
        evaluate()
      } else if (m.interface === OBJECTS && m.member === 'InterfacesRemoved' && typeof m.body[0] === 'string') {
        const removed = m.body[1]
        if (Array.isArray(removed) && removed.includes(ADAPTER)) {
          adapters.delete(m.body[0])
          if (scanning === m.body[0]) scanning = null
          evaluate()
        }
      } else if (m.interface === PROPS && m.member === 'PropertiesChanged' && m.path && typeof m.body[0] === 'string') {
        const changed = asProps(m.body[1])
        if (m.body[0] === DEVICE) serviceData(changed)
        else if (m.body[0] === ADAPTER && changed.has('Powered') && adapters.has(m.path)) {
          adapters.set(m.path, changed.get('Powered')!.value === true)
          evaluate()
        }
      }
    } catch (e) {
      console.error('Bluetooth: unexpected signal:', e instanceof Error ? e.message : e)
    }
  }

  const run = async (): Promise<void> => {
    let c: NonNullable<typeof client>
    try {
      c = await (options.connect ?? connect)()
    } catch (e) {
      console.error('Bluetooth: system bus unavailable:', e instanceof Error ? e.message : e)
      return state('denied')
    }
    if (stopped) return c.close()
    client = c
    c.onClose((e) => {
      if (!stopped) {
        if (e) console.error('Bluetooth: bus connection lost:', e.message)
        state('error')
      }
    })
    c.onSignal(onSignal)
    try {
      await Promise.all([
        // From BlueZ only: any other client of the bus could send these signals.
        c.addMatch(`type='signal',sender='${BLUEZ}',interface='${PROPS}',member='PropertiesChanged',path_namespace='/org/bluez'`),
        c.addMatch(`type='signal',sender='${BLUEZ}',interface='${OBJECTS}',member='InterfacesAdded'`),
        c.addMatch(`type='signal',sender='${BLUEZ}',interface='${OBJECTS}',member='InterfacesRemoved'`)
      ])
      const [objects] = await c.call({ destination: BLUEZ, path: '/', interface: OBJECTS, member: 'GetManagedObjects' })
      if (stopped) return
      for (const [path, ifaces] of objects as Map<string, Interfaces>) {
        addAdapter(ifaces, path)
        const dev = ifaces.get(DEVICE)
        if (dev) serviceData(dev)
      }
      evaluate()
    } catch (e) {
      // org.freedesktop.DBus.Error.ServiceUnknown: the Bluetooth service is not running, like no adapter.
      if (e instanceof DBusError && e.dbusName === 'org.freedesktop.DBus.Error.ServiceUnknown') return state('no-adapter')
      if (!stopped) fail(e, 'could not read the adapters')
    }
  }

  void run()

  return () => {
    if (stopped) return
    stopped = true
    const c = client
    const path = scanning
    client = null
    if (!c) return
    if (!path) return c.close()
    // Discovery ends with the connection anyway; asking first is polite.
    const timer = setTimeout(() => c.close(), STOP_WAIT)
    c.call({ destination: BLUEZ, path, interface: ADAPTER, member: 'StopDiscovery' })
      .catch(() => {})
      .finally(() => {
        clearTimeout(timer)
        c.close()
      })
  }
}

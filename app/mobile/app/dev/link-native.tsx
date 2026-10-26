// DEV ONLY: test bench for the native `signet-link` module (without the JS link logic). Driven by the route parameters,
// e.g. `/dev/link-native?a=pair&url=signet://link?d=...` (see scripts/link-test-desktop.mjs and the QA server).
// Actions (`a`) : pair, pair-badpin, control, ping, close, route, clear, browse, unbrowse, info.
import { useEffect } from 'react'
import { Text, View } from 'react-native'
import { useLocalSearchParams } from 'expo-router'
import { SignetLink, type PinnedEndpoint } from '../../modules/signet-link'

const log = (...a: unknown[]) => console.log('[link-native]', ...a)

interface Offer {
  desktop: { id: string; name: string }
  linkId: string
  certSha256: string
  code: string
  hosts: string[]
  port: number
}

// State kept between two commands.
let offer: Offer | null = null
let token: string | null = null
let seq = 0
const device = () => ({ id: 'link-native-test', name: SignetLink?.deviceName() ?? '?', platform: 'ios' })

if (__DEV__ && SignetLink) {
  SignetLink.addListener('onSocketLine', (e) => log('line', e.id, e.line))
  SignetLink.addListener('onSocketClosed', (e) => log('closed', e.id, e.error ?? 'normal'))
  SignetLink.addListener('onServiceFound', (e) => log('service found', JSON.stringify(e)))
  SignetLink.addListener('onServiceLost', (e) => log('service lost', e.name))
  SignetLink.addListener('onTunnelError', (e) => log('tunnel error', JSON.stringify(e)))
}

function decodeOffer(url: string): Offer {
  const b64 = url
    .slice(url.indexOf('d=') + 2)
    .replace(/-/g, '+')
    .replace(/_/g, '/')
  const bin = atob(b64)
  return JSON.parse(decodeURIComponent(escape(bin))) as Offer
}

const endpointFor = (o: Offer, host: string, sha = o.certSha256): PinnedEndpoint => ({
  host,
  port: o.port,
  serverName: `${o.linkId}.link.signet.invalid`,
  certSha256: sha
})

/** Tries the addresses in order; returns the id of the opened socket. */
async function openFirst(o: Offer, sha?: string): Promise<string> {
  let last: unknown
  for (const host of o.hosts) {
    const id = `s${++seq}`
    const t0 = Date.now()
    try {
      await SignetLink!.open(id, endpointFor(o, host, sha), 4000)
      log('open ok', id, host, `${Date.now() - t0}ms`)
      return id
    } catch (e) {
      last = e
      log('open failed', id, host, (e as { code?: string }).code ?? String(e), `${Date.now() - t0}ms`)
    }
  }
  throw last
}

let controlId: string | null = null

async function run(p: Record<string, string | undefined>): Promise<void> {
  if (!SignetLink) return log('module absent')
  const a = p.a
  log('action', a)
  if (a === 'info') {
    log('device', SignetLink.deviceName(), 'routing supported', SignetLink.isLocalhostRoutingSupported())
  } else if (a === 'pair' || a === 'pair-badpin') {
    if (p.url) offer = decodeOffer(p.url)
    if (!offer) return log('no pairing URL')
    const id = await openFirst(offer, a === 'pair-badpin' ? offer.certSha256.replace(/^./, (c) => (c === 'A' ? 'B' : 'A')) : undefined)
    await SignetLink.writeLine(id, JSON.stringify({ type: 'pair', v: 1, linkId: offer.linkId, code: offer.code, device: device() }))
    log('pair sent on', id)
  } else if (a === 'control') {
    if (!offer || !token) return log('no link (token:', token, ')')
    controlId = await openFirst(offer)
    await SignetLink.writeLine(controlId, JSON.stringify({ type: 'hello', v: 1, linkId: offer.linkId, token, role: 'control', device: device() }))
  } else if (a === 'ping') {
    if (controlId) await SignetLink.writeLine(controlId, JSON.stringify({ type: 'ping' }))
  } else if (a === 'close') {
    if (controlId) SignetLink.close(controlId)
    controlId = null
  } else if (a === 'route') {
    if (!offer || !token) return log('no link')
    // `host` forces the order of attempts (e.g. a dead address first to test the fallback).
    const hosts = p.host ? [p.host, ...offer.hosts] : offer.hosts
    // `ports`: relayed ports (e.g. `5173,3000`).
    const ports = (p.ports ?? '5173')
      .split(',')
      .map(Number)
      .filter((n) => n > 0)
    const ok = await SignetLink.setLocalhostRoute({ endpoints: hosts.map((h) => endpointFor(offer!, h)), linkId: offer.linkId, token, ports })
    log('route set ->', ok)
  } else if (a === 'clear') {
    log('route cleared ->', await SignetLink.setLocalhostRoute(null))
  } else if (a === 'browse') {
    SignetLink.startBrowsing()
  } else if (a === 'unbrowse') {
    SignetLink.stopBrowsing()
  }
}

// Remembers the token received at pairing.
if (__DEV__ && SignetLink) {
  SignetLink.addListener('onSocketLine', (e) => {
    try {
      const m = JSON.parse(e.line) as { type?: string; token?: string }
      if (m.type === 'paired' && m.token) token = m.token
    } catch {
      // ignored
    }
  })
}

export default function LinkNative() {
  const params = useLocalSearchParams<Record<string, string>>()
  const key = JSON.stringify(params)
  useEffect(() => {
    if (!__DEV__) return
    run(params).catch((e) => log('error', (e as { code?: string }).code ?? String(e)))
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [key])
  return (
    <View style={{ flex: 1, alignItems: 'center', justifyContent: 'center', padding: 24 }}>
      <Text>{String(params.a ?? '')}</Text>
    </View>
  )
}

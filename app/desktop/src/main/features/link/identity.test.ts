import { describe, expect, it } from 'vitest'
import { lanAddresses } from './identity'

const v4 = (address: string, internal = false) => ({ address, family: 'IPv4' as const, internal, netmask: '', mac: '', cidr: null })
const v6 = (address: string) => ({ address, family: 'IPv6' as const, internal: false, netmask: '', mac: '', cidr: null, scopeid: 0 })

describe('lanAddresses', () => {
  it('keeps Wi-Fi and Ethernet, without virtual or link-local networks', () => {
    expect(
      lanAddresses({
        lo0: [v4('127.0.0.1', true)],
        bridge100: [v4('192.168.139.3')],
        bridge101: [v4('192.168.97.0')],
        utun3: [v4('10.8.0.2')],
        en7: [v4('169.254.10.4')],
        en0: [v4('192.168.219.102'), v6('fd07::1'), v6('fe80::1')]
      })
    ).toEqual(['192.168.219.102'])
  })

  it('puts real interfaces first, three addresses at most', () => {
    expect(lanAddresses({ foo0: [v4('10.0.0.9')], eth0: [v4('10.0.0.1')], wlan0: [v4('10.0.0.2')], enp3s0: [v4('10.0.0.3')] })).toEqual([
      '10.0.0.1',
      '10.0.0.2',
      '10.0.0.3'
    ])
  })

  it('IPv6 only without IPv4', () => {
    expect(lanAddresses({ en0: [v6('fe80::1'), v6('fd07::1'), v6('2a01::1'), v6('2a01::2')] })).toEqual(['fd07::1', '2a01::1'])
  })
})

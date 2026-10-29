import { describe, expect, it } from 'vitest'
import { parseProxyConfig, toElectronProxy } from './proxyconfig'

const convert = (v: unknown) => toElectronProxy(parseProxyConfig(v))

describe('toElectronProxy', () => {
  it('maps the simple modes', () => {
    expect(convert({ mode: 'direct' })).toEqual({ mode: 'direct' })
    expect(convert({ mode: 'auto_detect' })).toEqual({ mode: 'auto_detect' })
    expect(convert({ mode: 'system' })).toEqual({ mode: 'system' })
  })

  it('uses a PAC url as is', () => {
    expect(convert({ mode: 'pac_script', pacScript: { url: 'https://example.com/proxy.pac' } })).toEqual({
      mode: 'pac_script',
      pacScript: 'https://example.com/proxy.pac'
    })
  })

  it('turns inline PAC data into a data: URL', () => {
    const script = 'function FindProxyForURL(){return "DIRECT";}'
    const out = convert({ mode: 'pac_script', pacScript: { data: script } })
    expect(out.pacScript).toBe(`data:application/x-ns-proxy-autoconfig;base64,${Buffer.from(script).toString('base64')}`)
  })

  it('single proxy covers every scheme, port optional, bypass list joined', () => {
    expect(
      convert({ mode: 'fixed_servers', rules: { singleProxy: { scheme: 'socks5', host: 'p.example.com', port: 1080 }, bypassList: ['<local>', '*.lan'] } })
    ).toEqual({
      mode: 'fixed_servers',
      proxyRules: 'socks5://p.example.com:1080',
      proxyBypassRules: '<local>,*.lan'
    })
    expect(convert({ mode: 'fixed_servers', rules: { singleProxy: { host: 'p.example.com' } } }).proxyRules).toBe('http://p.example.com')
  })

  it('per-scheme rules, fallback as socks entry, IPv6 bracketed', () => {
    const out = convert({
      mode: 'fixed_servers',
      rules: {
        proxyForHttp: { host: 'a.example.com', port: 80 },
        proxyForHttps: { scheme: 'https', host: 'b.example.com', port: 443 },
        proxyForFtp: { host: '::1', port: 21 },
        fallbackProxy: { scheme: 'socks4', host: 'c.example.com', port: 1080 }
      }
    })
    expect(out.proxyRules).toBe('http=http://a.example.com:80;https=https://b.example.com:443;ftp=http://[::1]:21;socks=socks4://c.example.com:1080')
    expect(out.proxyBypassRules).toBeUndefined()
  })
})

describe('parseProxyConfig', () => {
  it.each([
    [null],
    [{}],
    [{ mode: 'nope' }],
    [{ mode: 'fixed_servers' }],
    [{ mode: 'fixed_servers', rules: {} }],
    [{ mode: 'fixed_servers', rules: { singleProxy: { host: 'a;b=c' } } }],
    [{ mode: 'fixed_servers', rules: { singleProxy: { host: 'a', port: 70000 } } }],
    [{ mode: 'fixed_servers', rules: { singleProxy: { host: 'a', scheme: 'ftp' } } }],
    [{ mode: 'fixed_servers', rules: { singleProxy: { host: 'a' }, bypassList: ['a,b'] } }],
    [{ mode: 'pac_script' }],
    [{ mode: 'pac_script', pacScript: {} }]
  ])('rejects %j', (value) => {
    expect(() => parseProxyConfig(value)).toThrow()
  })

  it('keeps a clean copy (unknown properties dropped)', () => {
    const out = parseProxyConfig({ mode: 'fixed_servers', evil: 1, rules: { singleProxy: { host: 'a', x: 1 } } })
    expect(out).toEqual({ mode: 'fixed_servers', rules: { singleProxy: { host: 'a' } } })
  })
})

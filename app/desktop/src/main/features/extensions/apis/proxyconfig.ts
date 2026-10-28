import type { ProxyConfig } from 'electron'
import { ApiError } from './types'

/** Chrome's `proxy.ProxyConfig`, as validated and stored (JSON-safe). */
export interface ChromeProxyConfig {
  mode: 'direct' | 'auto_detect' | 'pac_script' | 'fixed_servers' | 'system'
  rules?: ProxyRules
  pacScript?: { url?: string; data?: string; mandatory?: boolean }
}

export interface ProxyRules {
  singleProxy?: ProxyServer
  proxyForHttp?: ProxyServer
  proxyForHttps?: ProxyServer
  proxyForFtp?: ProxyServer
  fallbackProxy?: ProxyServer
  bypassList?: string[]
}

export interface ProxyServer {
  scheme?: 'http' | 'https' | 'quic' | 'socks4' | 'socks5'
  host: string
  port?: number
}

const MODES = ['direct', 'auto_detect', 'pac_script', 'fixed_servers', 'system'] as const
const SCHEMES = ['http', 'https', 'quic', 'socks4', 'socks5'] as const
const SERVER_KEYS = ['singleProxy', 'proxyForHttp', 'proxyForHttps', 'proxyForFtp', 'fallbackProxy'] as const

/** What `proxy.settings` reports when no extension controls it: the system proxy. */
export const SYSTEM_PROXY: ChromeProxyConfig = { mode: 'system' }

const isObject = (v: unknown): v is Record<string, unknown> => !!v && typeof v === 'object' && !Array.isArray(v)

function parseServer(value: unknown, key: string): ProxyServer {
  if (!isObject(value)) throw new ApiError(`Invalid value for argument 1. Property 'rules.${key}': Expected 'object' but got '${typeof value}'.`)
  const { host, port, scheme } = value
  // Rules are joined into a `;` separated string: reject anything that could add a rule or a scheme.
  if (typeof host !== 'string' || !host || /[\s;,=/\\]/.test(host))
    throw new ApiError(`Invalid value for argument 1. Property 'rules.${key}.host': Invalid host.`)
  const out: ProxyServer = { host }
  if (port !== undefined) {
    if (typeof port !== 'number' || !Number.isInteger(port) || port < 1 || port > 65535) {
      throw new ApiError(`Invalid value for argument 1. Property 'rules.${key}.port': Invalid port.`)
    }
    out.port = port
  }
  if (scheme !== undefined) {
    if (!SCHEMES.includes(scheme as (typeof SCHEMES)[number]))
      throw new ApiError(`Invalid value for argument 1. Property 'rules.${key}.scheme': Invalid scheme.`)
    out.scheme = scheme as ProxyServer['scheme']
  }
  return out
}

/** Validates an extension-supplied ProxyConfig and returns a clean copy; throws an `ApiError` the extension can read. */
export function parseProxyConfig(value: unknown): ChromeProxyConfig {
  if (!isObject(value)) throw new ApiError("Invalid value for argument 1. Property 'value': Expected 'object'.")
  const mode = value.mode
  if (!MODES.includes(mode as (typeof MODES)[number])) {
    throw new ApiError(`Invalid value for argument 1. Property 'mode': Value must be one of: [${MODES.join(', ')}].`)
  }
  const config: ChromeProxyConfig = { mode: mode as ChromeProxyConfig['mode'] }
  if (mode === 'fixed_servers') {
    const rules = value.rules
    if (!isObject(rules)) throw new ApiError("Invalid value for argument 1. 'rules' is required for fixed_servers mode.")
    const clean: ProxyRules = {}
    for (const key of SERVER_KEYS) if (rules[key] !== undefined) clean[key] = parseServer(rules[key], key)
    if (rules.bypassList !== undefined) {
      const list = rules.bypassList
      if (!Array.isArray(list) || list.some((p) => typeof p !== 'string' || !p || /[\s,;]/.test(p))) {
        throw new ApiError("Invalid value for argument 1. Property 'rules.bypassList': Expected 'array' of non-empty strings without spaces.")
      }
      clean.bypassList = list as string[]
    }
    if (!SERVER_KEYS.some((k) => clean[k])) throw new ApiError('Invalid value for argument 1. No proxy server given in rules.')
    config.rules = clean
  } else if (mode === 'pac_script') {
    const pac = value.pacScript
    if (!isObject(pac)) throw new ApiError("Invalid value for argument 1. 'pacScript' is required for pac_script mode.")
    const { url, data, mandatory } = pac
    if (typeof url !== 'string' && typeof data !== 'string') throw new ApiError("Invalid value for argument 1. 'pacScript' needs a 'url' or 'data'.")
    if ((typeof url === 'string' && !url) || (typeof data === 'string' && !data)) throw new ApiError("Invalid value for argument 1. 'pacScript' is empty.")
    config.pacScript = {
      ...(typeof url === 'string' && { url }),
      ...(typeof data === 'string' && { data }),
      ...(typeof mandatory === 'boolean' && { mandatory })
    }
  }
  return config
}

/** Chromium's textual form of one proxy: `scheme://host:port` (the port is left out when absent, Chromium then picks the scheme's default). */
function serverRule(server: ProxyServer): string {
  // IPv6 literals need brackets in a URL-like rule.
  const host = server.host.includes(':') && !server.host.startsWith('[') ? `[${server.host}]` : server.host
  return `${server.scheme ?? 'http'}://${host}${server.port ? `:${server.port}` : ''}`
}

/**
 * Chrome ProxyConfig -> `session.setProxy` argument.
 * - pac_script with inline `data` becomes a `data:` URL, as Chrome's own proxy API does (Chromium's PAC fetcher accepts
 *   data URLs); the `mandatory` flag is not supported (Chromium has no way to refuse direct connections on PAC failure).
 * - fixed_servers: `singleProxy` applies to every scheme; otherwise per-scheme rules, `fallbackProxy` being Chromium's
 *   `socks=` entry (used for the schemes with no rule of their own).
 */
export function toElectronProxy(config: ChromeProxyConfig): ProxyConfig {
  switch (config.mode) {
    case 'direct':
      return { mode: 'direct' }
    case 'auto_detect':
      return { mode: 'auto_detect' }
    case 'system':
      return { mode: 'system' }
    case 'pac_script': {
      const { url, data } = config.pacScript ?? {}
      const pacScript = url ?? `data:application/x-ns-proxy-autoconfig;base64,${Buffer.from(data ?? '', 'utf8').toString('base64')}`
      return { mode: 'pac_script', pacScript }
    }
    case 'fixed_servers': {
      const rules = config.rules ?? {}
      let proxyRules: string
      if (rules.singleProxy) proxyRules = serverRule(rules.singleProxy)
      else {
        const parts: string[] = []
        if (rules.proxyForHttp) parts.push(`http=${serverRule(rules.proxyForHttp)}`)
        if (rules.proxyForHttps) parts.push(`https=${serverRule(rules.proxyForHttps)}`)
        if (rules.proxyForFtp) parts.push(`ftp=${serverRule(rules.proxyForFtp)}`)
        if (rules.fallbackProxy) parts.push(`socks=${serverRule(rules.fallbackProxy)}`)
        proxyRules = parts.join(';')
      }
      return { mode: 'fixed_servers', proxyRules, ...(rules.bypassList?.length && { proxyBypassRules: rules.bypassList.join(',') }) }
    }
  }
}

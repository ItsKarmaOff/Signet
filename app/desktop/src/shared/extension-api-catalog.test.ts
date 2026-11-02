import probeManifest from '../../tests/extensions/api-probe/manifest.json'
import { describe, expect, it } from 'vitest'
import { EXTENSION_API_CATALOG, TYPE_ONLY_NAMESPACES, catalogPaths, catalogPermissions, type NamespaceSpec } from './extension-api-catalog'

/** Every namespace of the inventory table (section 1 of the extension API inventory study): catalog entry (dotted for children) or type-only. */
const INVENTORY = [
  'action',
  'alarms',
  'commands',
  'contextMenus',
  'cookies',
  'declarativeNetRequest',
  'downloads',
  'i18n',
  'notifications',
  'offscreen',
  'permissions',
  'privacy',
  'proxy',
  'runtime',
  'scripting',
  'storage.local',
  'storage.session',
  'storage.sync',
  'storage.managed',
  'tabs',
  'webNavigation',
  'webRequest',
  'windows',
  'bookmarks',
  'browsingData',
  'contentSettings',
  'declarativeContent',
  'desktopCapture',
  'devtools.inspectedWindow',
  'devtools.network',
  'devtools.panels',
  'devtools.performance',
  'devtools.recorder',
  'dom',
  'extension',
  'fontSettings',
  'history',
  'identity',
  'idle',
  'management',
  'omnibox',
  'pageCapture',
  'power',
  'search',
  'sessions',
  'sidePanel',
  'system.cpu',
  'system.memory',
  'system.display',
  'system.storage',
  'tabCapture',
  'tabGroups',
  'topSites',
  'tts',
  'userScripts',
  'accessibilityFeatures',
  'audio',
  'browserAction',
  'pageAction',
  'certificateProvider',
  'debugger',
  'dns',
  'documentScan',
  'enterprise.deviceAttributes',
  'enterprise.hardwarePlatform',
  'enterprise.login',
  'enterprise.networkingAttributes',
  'enterprise.platformKeys',
  'events',
  'types',
  'extensionTypes',
  'fileBrowserHandler',
  'fileSystemProvider',
  'gcm',
  'instanceID',
  'input.ime',
  'loginState',
  'mimeHandler',
  'platformKeys',
  'printerProvider',
  'printing',
  'printingMetrics',
  'processes',
  'publicSuffix',
  'readingList',
  'systemLog',
  'ttsEngine',
  'vpnProvider',
  'wallpaper',
  'webAuthenticationProxy',
  'clipboard'
]

const everySpec = (c: Record<string, NamespaceSpec> = EXTENSION_API_CATALOG): NamespaceSpec[] =>
  Object.values(c).flatMap((s) => [s, ...everySpec(s.children ?? {})])

describe('extension API catalog', () => {
  const paths = catalogPaths()

  it('covers every namespace of the inventory (or lists it as type-only)', () => {
    const missing = INVENTORY.filter((n) => !paths.includes(n) && !TYPE_ONLY_NAMESPACES.includes(n))
    expect(missing).toEqual([])
  })

  it('has no duplicate namespace path', () => {
    expect(new Set(paths).size).toBe(paths.length)
  })

  it('does not list a type-only namespace as a real one', () => {
    for (const n of TYPE_ONLY_NAMESPACES) expect(paths).not.toContain(n)
  })

  it('has valid shapes', () => {
    const isNames = (a: unknown) => Array.isArray(a) && a.every((x) => typeof x === 'string' && /^[A-Za-z_$][\w$]*$/.test(x))
    for (const [path, spec] of Object.entries(
      Object.fromEntries(
        paths.map((p) => [p, p.split('.').reduce<NamespaceSpec>((s, k, i) => (i === 0 ? EXTENSION_API_CATALOG[k] : s.children![k]), undefined as never)])
      )
    )) {
      expect(spec, path).toBeDefined()
      expect(Array.isArray(spec.permissions), path).toBe(true)
      expect(
        spec.permissions.every((p) => typeof p === 'string' && p.length > 0),
        path
      ).toBe(true)
      expect(isNames(spec.methods), `${path} methods`).toBe(true)
      expect(isNames(spec.events), `${path} events`).toBe(true)
      if (spec.declarativeEvents) for (const e of spec.declarativeEvents) expect(spec.events, `${path} declarative ${e}`).toContain(e)
      for (const k of spec.classes ?? []) expect(isNames([k]), `${path} class ${k}`).toBe(true)
      for (const m of Object.keys(spec.soft ?? {})) expect(spec.methods, `${path} soft ${m}`).toContain(m)
      expect(new Set(spec.methods).size, `${path} methods unique`).toBe(spec.methods.length)
      expect(new Set(spec.events).size, `${path} events unique`).toBe(spec.events.length)
    }
  })

  it('is plain JSON data (it travels to the preload and the probe as JSON)', () => {
    expect(JSON.parse(JSON.stringify(EXTENSION_API_CATALOG))).toEqual(EXTENSION_API_CATALOG)
  })

  it('flags only the namespaces Signet will never provide', () => {
    const flagged = Object.entries(EXTENSION_API_CATALOG)
      .filter(([, s]) => s.unsupported)
      .map(([n]) => n)
    for (const n of ['gcm', 'instanceID', 'enterprise', 'tabCapture', 'debugger', 'mimeHandler', 'readingList', 'ttsEngine']) expect(flagged).toContain(n)
    for (const n of ['tabs', 'storage', 'downloads', 'sidePanel', 'identity', 'bookmarks']) expect(flagged).not.toContain(n)
  })

  it('has at least one member somewhere for every supported top-level namespace except the empty containers', () => {
    const empty = new Set(['publicSuffix', 'system', 'devtools', 'storage', 'privacy', 'proxy', 'contentSettings'])
    for (const [n, s] of Object.entries(EXTENSION_API_CATALOG)) {
      if (s.unsupported || empty.has(n)) continue
      expect(s.methods.length + s.events.length + (s.classes?.length ?? 0), n).toBeGreaterThan(0)
    }
  })

  it('is fully requested by the probe manifest (required or optional)', () => {
    const manifest = probeManifest as unknown as {
      permissions: string[]
      optional_permissions: string[]
      host_permissions: string[]
      action: { default_popup: string }
    } & Record<string, unknown>
    const requested = new Set([...manifest.permissions, ...manifest.optional_permissions])
    expect(catalogPermissions().filter((p) => !requested.has(p))).toEqual([])
    expect(manifest.optional_permissions.length).toBeGreaterThan(0)
    expect(manifest.permissions.filter((p) => manifest.optional_permissions.includes(p))).toEqual([])
    expect(manifest.host_permissions).toContain('<all_urls>')
    for (const key of ['commands', 'side_panel', 'omnibox', 'options_page']) expect(manifest, key).toHaveProperty(key)
    expect(manifest.action.default_popup).toBe('popup.html')
    expect(everySpec().length).toBeGreaterThan(60)
  })
})

import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type { ExtensionActionCounts } from '@shared/types'
import type { Session } from 'electron'
import { DnrStore } from './dnr'
import { DnrEngine } from './dnr-engine'
import { NO_CONTEXT, type ApiCaller } from './types'

const ID = 'a'.repeat(32)
const OTHER = 'b'.repeat(32)

let dir: string
beforeEach(() => {
  dir = mkdtempSync(join(tmpdir(), 'signet-dnr-engine-'))
})
afterEach(() => rmSync(dir, { recursive: true, force: true, maxRetries: 5, retryDelay: 50 }))

interface Ext {
  id: string
  path: string
  version: string
  manifest: Record<string, unknown>
}

/** A session with Electron's single-listener `webRequest` and an extension registry with events. */
function fakeSession(exts: Ext[] = []) {
  const listeners: Record<string, ((...a: never[]) => void) | null> = {}
  const webRequest = new Proxy(
    {},
    { get: (_t, event: string) => (a: unknown, b?: (...x: never[]) => void) => void (listeners[event] = a === null ? null : (b ?? null)) }
  )
  const handlers: Record<string, ((...a: unknown[]) => void)[]> = {}
  const extensions = {
    getAllExtensions: () => exts,
    getExtension: (id: string) => exts.find((e) => e.id === id) ?? null,
    on: (event: string, fn: (...a: unknown[]) => void) => void (handlers[event] ??= []).push(fn)
  }
  const ses = { webRequest, extensions } as unknown as Session
  const fire = (event: string, ext: Ext) => handlers[event]?.forEach((fn) => fn({}, ext))
  return { ses, listeners, exts, fire }
}

const run = (l: ((...a: never[]) => void) | null | undefined, details: object): Promise<Record<string, unknown>> =>
  new Promise((resolve) => (l as unknown as (d: object, cb: (r: Record<string, unknown>) => void) => void)(details, resolve))

const details = (url: string, extra: object = {}) => ({ url, method: 'GET', resourceType: 'script', webContentsId: 1, referrer: 'https://site.org/', ...extra })

function writeExt(name: string, manifestExtra: Record<string, unknown>, rulesets: Record<string, unknown>): Ext {
  const path = join(dir, name)
  mkdirSync(join(path, 'rules'), { recursive: true })
  for (const [file, content] of Object.entries(rulesets))
    writeFileSync(join(path, 'rules', file), typeof content === 'string' ? content : JSON.stringify(content))
  const manifest = { permissions: ['declarativeNetRequest'], ...manifestExtra }
  writeFileSync(join(path, 'manifest.json'), JSON.stringify(manifest))
  return { id: name === 'one' ? ID : OTHER, path, version: '1.0', manifest }
}

const block = (id: number, urlFilter: string, extra: Record<string, unknown> = {}) => ({
  id,
  priority: 1,
  action: { type: 'block' },
  condition: { urlFilter, ...extra }
})

function setup(ext: Ext, logs: string[] = [], extra: Partial<ConstructorParameters<typeof DnrEngine>[1]> = {}) {
  const store = new DnrStore(join(dir, 'dnr.json'))
  const engine = new DnrEngine(store, { log: (m) => logs.push(m), now: () => 1_000_000, ...extra })
  const s = fakeSession([ext])
  engine.attach(s.ses)
  const caller: ApiCaller = { ses: s.ses, extensionId: ext.id, context: NO_CONTEXT }
  const call = (method: string, ...args: unknown[]) => engine.methods()[`declarativeNetRequest.${method}`](caller, ...args)
  /** Rebuilds until static rulesets are loaded. */
  const ready = async (check: () => boolean) => {
    for (let i = 0; i < 100; i++) {
      engine.rebuild()
      if (check()) return
      await new Promise((r) => setTimeout(r, 10))
    }
    throw new Error('rulesets not loaded')
  }
  return { store, engine, s, call, ready, caller }
}

describe('static rulesets', () => {
  const ext = () =>
    writeExt(
      'one',
      {
        declarative_net_request: {
          rule_resources: [
            { id: 'ads', enabled: true, path: 'rules/ads.json' },
            { id: 'extra', enabled: false, path: 'rules/extra.json' }
          ]
        }
      },
      {
        'ads.json': [block(1, '||ads.example^'), { id: 2, action: { type: 'NOPE' }, condition: {} }, block(3, 'x', { regexFilter: 'y' })],
        'extra.json': [block(10, '||extra.example^')]
      }
    )

  it('loads the enabled rulesets, skips invalid rules with one log line, and blocks', async () => {
    const logs: string[] = []
    const t = setup(ext(), logs)
    await t.ready(() => t.s.listeners.onBeforeRequest !== null && t.s.listeners.onBeforeRequest !== undefined)
    expect(await run(t.s.listeners.onBeforeRequest, details('https://ads.example/a.js'))).toEqual({ cancel: true })
    expect(await run(t.s.listeners.onBeforeRequest, details('https://extra.example/a.js'))).toEqual({})
    expect(await run(t.s.listeners.onBeforeRequest, details('https://ok.example/a.js'))).toEqual({})
    expect(logs.filter((l) => l.includes('skipped 2 invalid rule(s)'))).toHaveLength(1)
  })

  it('reports and updates the enabled set, persists it and applies it at once', async () => {
    const e = ext()
    const t = setup(e)
    await t.ready(() => !!t.s.listeners.onBeforeRequest)
    expect(t.call('getEnabledRulesets')).toEqual(['ads'])
    await t.call('updateEnabledRulesets', { enableRulesetIds: ['extra'], disableRulesetIds: ['ads'] })
    expect(t.call('getEnabledRulesets')).toEqual(['extra'])
    expect(await run(t.s.listeners.onBeforeRequest, details('https://extra.example/a.js'))).toEqual({ cancel: true })
    expect(await run(t.s.listeners.onBeforeRequest, details('https://ads.example/a.js'))).toEqual({})
    t.store.flush()
    const again = new DnrStore(join(dir, 'dnr.json'))
    expect(again.enabledRulesets(e.id, '1.0')).toEqual(['extra'])
    expect(again.enabledRulesets(e.id, '2.0')).toBeUndefined()
  })

  it('rejects unknown rulesets and counts what is left', async () => {
    const t = setup(ext())
    await t.ready(() => !!t.s.listeners.onBeforeRequest)
    await expect(t.call('updateEnabledRulesets', { enableRulesetIds: ['nope'] })).rejects.toThrow(/Invalid ruleset id/)
    expect(t.call('getAvailableStaticRuleCount')).toBe(330_000 - 1)
  })

  it('reads a manifest path written with a leading slash inside the extension', async () => {
    const e = writeExt(
      'slash',
      { declarative_net_request: { rule_resources: [{ id: 'ads', enabled: true, path: '/rules/ads.json' }] } },
      { 'ads.json': [block(1, '||ads.example^')] }
    )
    const t = setup(e)
    await t.ready(() => !!t.s.listeners.onBeforeRequest)
    expect(await run(t.s.listeners.onBeforeRequest, details('https://ads.example/a.js'))).toEqual({ cancel: true })
  })

  it('refuses a ruleset path outside the extension and survives a broken file', async () => {
    const logs: string[] = []
    const e = writeExt(
      'one',
      {
        declarative_net_request: {
          rule_resources: [
            { id: 'out', enabled: true, path: '../escape.json' },
            { id: 'bad', enabled: true, path: 'rules/bad.json' }
          ]
        }
      },
      { 'bad.json': '{ not json' }
    )
    const t = setup(e, logs)
    await t.ready(() => logs.length >= 2)
    expect(logs.some((l) => l.includes('outside the extension'))).toBe(true)
    expect(logs.some((l) => l.includes('"bad" cannot be loaded'))).toBe(true)
  })

  it('stops enforcing when the extension is unloaded', async () => {
    const e = ext()
    const t = setup(e)
    await t.ready(() => !!t.s.listeners.onBeforeRequest)
    t.s.exts.length = 0
    t.s.fire('extension-unloaded', e)
    t.engine.rebuild()
    expect(t.s.listeners.onBeforeRequest).toBeNull()
  })

  it('does nothing without the declarativeNetRequest permission', async () => {
    const e = ext()
    e.manifest = { ...e.manifest, permissions: [] }
    const t = setup(e)
    t.engine.rebuild()
    expect(t.s.listeners.onBeforeRequest).toBeUndefined()
  })
})

describe('hub wiring', () => {
  const bare = () =>
    writeExt(
      'one',
      { permissions: ['declarativeNetRequest', 'declarativeNetRequestFeedback'], host_permissions: ['*://*.site.org/*', '*://*.example.com/*'] },
      {}
    )
  const headerRule = (id: number, key: 'requestHeaders' | 'responseHeaders') => ({
    id,
    priority: 1,
    action: { type: 'modifyHeaders', [key]: [{ header: 'X-Test', operation: 'set', value: 'yes' }] },
    condition: { urlFilter: '||example.com^' }
  })

  it('takes onBeforeSendHeaders only while an active rule changes request headers', () => {
    const t = setup(bare())
    t.engine.rebuild()
    expect(t.s.listeners.onBeforeRequest).toBeUndefined()
    expect(t.s.listeners.onBeforeSendHeaders).toBeUndefined()
    t.store.update('session', ID, { addRules: [block(1, '||ads.example^')] })
    t.engine.rebuild()
    expect(t.s.listeners.onBeforeRequest).toBeTruthy()
    expect(t.s.listeners.onBeforeSendHeaders).toBeUndefined()
    expect(t.s.listeners.onHeadersReceived).toBeUndefined()
    t.store.update('dynamic', ID, { addRules: [headerRule(2, 'requestHeaders')] })
    t.engine.rebuild()
    expect(t.s.listeners.onBeforeSendHeaders).toBeTruthy()
    expect(t.s.listeners.onHeadersReceived).toBeUndefined()
    expect(t.engine.registered(t.s.ses).sort()).toEqual(['onBeforeRequest', 'onBeforeSendHeaders'])
    t.store.update('dynamic', ID, { removeRuleIds: [2] })
    t.engine.rebuild()
    expect(t.s.listeners.onBeforeSendHeaders).toBeNull()
  })

  it('rebuilds by itself after a rule change (debounced)', async () => {
    const t = setup(bare())
    t.store.update('session', ID, { addRules: [block(1, '||ads.example^')] })
    await new Promise((r) => setTimeout(r, 120))
    expect(await run(t.s.listeners.onBeforeRequest, details('https://ads.example/x'))).toEqual({ cancel: true })
  })

  it('changes request and response headers', async () => {
    const t = setup(bare())
    t.store.update('dynamic', ID, { addRules: [headerRule(1, 'requestHeaders'), headerRule(2, 'responseHeaders')] })
    t.engine.rebuild()
    const req = await run(t.s.listeners.onBeforeSendHeaders, { ...details('https://example.com/a'), requestHeaders: { Accept: '*/*' } })
    expect(req.requestHeaders).toEqual({ Accept: '*/*', 'X-Test': 'yes' })
    const res = await run(t.s.listeners.onHeadersReceived, { ...details('https://example.com/a'), responseHeaders: { 'Content-Type': ['text/html'] } })
    expect(res.responseHeaders).toEqual({ 'Content-Type': ['text/html'], 'X-Test': ['yes'] })
    expect(await run(t.s.listeners.onBeforeSendHeaders, { ...details('https://other.net/a'), requestHeaders: { A: '1' } })).toEqual({})
  })

  it('redirects only with host access to the URL and the initiator', async () => {
    const redirect = {
      id: 1,
      priority: 1,
      action: { type: 'redirect', redirect: { url: 'https://safe.example/' } },
      condition: { urlFilter: '||example.com^' }
    }
    const withHosts = setup(bare())
    withHosts.store.update('dynamic', ID, { addRules: [redirect] })
    withHosts.engine.rebuild()
    expect(await run(withHosts.s.listeners.onBeforeRequest, details('https://example.com/a'))).toEqual({ redirectURL: 'https://safe.example/' })
    expect(await run(withHosts.s.listeners.onBeforeRequest, details('https://example.com/a', { referrer: 'https://elsewhere.net/' }))).toEqual({})
    const noHosts = setup(writeExt('one', { permissions: ['declarativeNetRequest'] }, {}))
    noHosts.store.update('dynamic', ID, { addRules: [redirect, block(2, '||blocked.example^')] })
    noHosts.engine.rebuild()
    expect(await run(noHosts.s.listeners.onBeforeRequest, details('https://example.com/a'))).toEqual({})
    expect(await run(noHosts.s.listeners.onBeforeRequest, details('https://blocked.example/a'))).toEqual({ cancel: true })
  })

  it('upgrades the scheme and applies main-frame rules', async () => {
    const t = setup(bare())
    t.store.update('session', ID, {
      addRules: [
        { id: 1, priority: 1, action: { type: 'upgradeScheme' }, condition: { urlFilter: '||example.com^', resourceTypes: ['main_frame'] } },
        block(2, '||ads.example^', { resourceTypes: ['main_frame'] })
      ]
    })
    t.engine.rebuild()
    const main = { resourceType: 'mainFrame', referrer: '' }
    expect(await run(t.s.listeners.onBeforeRequest, details('http://example.com/', main))).toEqual({ redirectURL: 'https://example.com/' })
    expect(await run(t.s.listeners.onBeforeRequest, details('https://ads.example/', main))).toEqual({ cancel: true })
    expect(await run(t.s.listeners.onBeforeRequest, details('https://ads.example/'))).toEqual({})
  })

  it('ignores requests made by another extension', async () => {
    const t = setup(bare())
    t.store.update('session', ID, { addRules: [block(1, '||ads.example^')] })
    t.engine.rebuild()
    const own = { referrer: '', frame: { origin: `chrome-extension://${ID}` } }
    const foreign = { referrer: '', frame: { origin: `chrome-extension://${OTHER}` } }
    expect(await run(t.s.listeners.onBeforeRequest, details('https://ads.example/', own))).toEqual({ cancel: true })
    expect(await run(t.s.listeners.onBeforeRequest, details('https://ads.example/', foreign))).toEqual({})
  })

  it('keeps allowAllRequests for the tab until the next navigation', async () => {
    const t = setup(bare())
    t.store.update('session', ID, {
      addRules: [
        { id: 1, priority: 10, action: { type: 'allowAllRequests' }, condition: { urlFilter: '||trusted.org^', resourceTypes: ['main_frame'] } },
        block(2, '||ads.example^')
      ]
    })
    t.engine.rebuild()
    const nav = (url: string, tab = 1) => run(t.s.listeners.onBeforeRequest, details(url, { resourceType: 'mainFrame', referrer: '', webContentsId: tab }))
    const sub = (tab = 1) => run(t.s.listeners.onBeforeRequest, details('https://ads.example/a.js', { webContentsId: tab }))
    expect(await sub()).toEqual({ cancel: true })
    await nav('https://trusted.org/')
    expect(await sub()).toEqual({})
    expect(await sub(2)).toEqual({ cancel: true })
    await nav('https://other.org/')
    expect(await sub()).toEqual({ cancel: true })
  })

  it('logs matches for extensions with the feedback permission', async () => {
    const t = setup(bare())
    t.store.update('session', ID, { addRules: [block(7, '||ads.example^')] })
    t.engine.rebuild()
    await run(t.s.listeners.onBeforeRequest, details('https://ads.example/a.js', { webContentsId: 3 }))
    await run(t.s.listeners.onBeforeRequest, details('https://fine.example/a.js', { webContentsId: 3 }))
    const info = t.call('getMatchedRules', { tabId: 3 }) as { rulesMatchedInfo: unknown[] }
    expect(info.rulesMatchedInfo).toEqual([{ rule: { ruleId: 7, rulesetId: '_session' }, tabId: 3, timeStamp: 1_000_000 }])
    expect((t.call('getMatchedRules', { tabId: 4 }) as { rulesMatchedInfo: unknown[] }).rulesMatchedInfo).toEqual([])
    expect((t.call('getMatchedRules', { minTimeStamp: 2_000_000 }) as { rulesMatchedInfo: unknown[] }).rulesMatchedInfo).toEqual([])
    const plain = setup(writeExt('one', { permissions: ['declarativeNetRequest'] }, {}))
    expect(() => plain.call('getMatchedRules')).toThrow(/declarativeNetRequestFeedback/)
  })

  it('answers testMatchOutcome', () => {
    const t = setup(bare())
    t.store.update('session', ID, { addRules: [block(7, '||ads.example^')] })
    t.engine.rebuild()
    expect(t.call('testMatchOutcome', { url: 'https://ads.example/x', type: 'script' })).toEqual({ matchedRules: [{ ruleId: 7, rulesetId: '_session' }] })
    expect(t.call('testMatchOutcome', { url: 'https://fine.example/x', type: 'script' })).toEqual({ matchedRules: [] })
  })
})

describe('runtime grants', () => {
  it('optional host permissions and permissions only count once granted', () => {
    const granted = { perms: [] as string[], origins: [] as string[] }
    const grants = { permissionsOf: () => granted.perms, originsOf: () => granted.origins }
    const e = writeExt('one', { permissions: [], optional_permissions: ['declarativeNetRequestWithHostAccess'], optional_host_permissions: ['<all_urls>'] }, {})
    const t = setup(e, [], { grants })
    t.store.update('session', ID, { addRules: [block(1, '||ads.example^')] })
    const outcome = () => t.call('testMatchOutcome', { url: 'https://ads.example/x', type: 'script' })
    t.engine.rebuild()
    expect(t.engine.actionCounts()).toEqual({})
    expect(outcome()).toEqual({ matchedRules: [] })
    granted.perms = ['declarativeNetRequestWithHostAccess']
    t.engine.refreshAccess(ID)
    t.engine.rebuild()
    expect(outcome()).toEqual({ matchedRules: [] })
    granted.origins = ['https://ads.example/*']
    t.engine.refreshAccess(ID)
    t.engine.rebuild()
    expect(outcome()).toEqual({ matchedRules: [{ ruleId: 1, rulesetId: '_session' }] })
  })
})

describe('action count badge', () => {
  const MAIN = { resourceType: 'mainFrame', referrer: '' }
  const bare = () => writeExt('one', { host_permissions: ['<all_urls>'] }, {})
  function counting() {
    const pushed: ExtensionActionCounts[] = []
    const watched: Record<number, () => void> = {}
    const t = setup(bare(), [], { onCounts: (c) => pushed.push(c), watchTab: (id, fn) => void (watched[id] = fn) })
    t.store.update('session', ID, {
      addRules: [
        block(1, '||ads.example^'),
        { id: 2, priority: 1, action: { type: 'redirect', redirect: { url: 'https://safe.example/x' } }, condition: { urlFilter: '||redir.example^' } },
        { id: 3, priority: 1, action: { type: 'upgradeScheme' }, condition: { urlFilter: '||up.example^' } },
        { id: 4, priority: 5, action: { type: 'allow' }, condition: { urlFilter: '||fine.example^' } }
      ]
    })
    t.engine.rebuild()
    return { ...t, pushed, watched }
  }
  const req = (t: ReturnType<typeof counting>, url: string, extra: object = {}) => run(t.s.listeners.onBeforeRequest, details(url, extra))

  beforeEach(() => vi.useFakeTimers())
  afterEach(() => vi.useRealTimers())

  it('counts blocked, redirected and upgraded requests only when the option is on', async () => {
    const t = counting()
    await req(t, 'https://ads.example/a.js')
    expect(t.engine.actionCounts()).toEqual({})
    await t.call('setExtensionActionOptions', { displayActionCountAsBadgeText: true })
    await req(t, 'https://ads.example/a.js')
    await req(t, 'https://redir.example/a.js')
    await req(t, 'http://up.example/a.js')
    await req(t, 'https://fine.example/a.js')
    await req(t, 'https://ok.example/a.js')
    expect(t.engine.actionCounts()).toEqual({ 1: { [ID]: 3 } })
    await req(t, 'https://ads.example/a.js', { webContentsId: 2 })
    expect(t.engine.actionCounts()).toEqual({ 1: { [ID]: 3 }, 2: { [ID]: 1 } })
  })

  it('resets a tab on a main frame request and drops it with the tab', async () => {
    const t = counting()
    await t.call('setExtensionActionOptions', { displayActionCountAsBadgeText: true })
    await req(t, 'https://ads.example/a.js')
    await req(t, 'https://ads.example/b.js', { webContentsId: 2 })
    await req(t, 'https://ok.example/', MAIN)
    expect(t.engine.actionCounts()).toEqual({ 2: { [ID]: 1 } })
    t.watched[2]()
    expect(t.engine.actionCounts()).toEqual({})
  })

  it('adds tabUpdate increments, floors at 0 and validates', async () => {
    const t = counting()
    await t.call('setExtensionActionOptions', { displayActionCountAsBadgeText: true })
    await t.call('setExtensionActionOptions', { tabUpdate: { tabId: 7, increment: 5 } })
    await t.call('setExtensionActionOptions', { tabUpdate: { tabId: 7, increment: -2 } })
    expect(t.engine.actionCounts()).toEqual({ 7: { [ID]: 3 } })
    await t.call('setExtensionActionOptions', { tabUpdate: { tabId: 7, increment: -10 } })
    expect(t.engine.actionCounts()).toEqual({})
    expect(() => t.call('setExtensionActionOptions', { tabUpdate: { tabId: 'x', increment: 1 } })).toThrow(/Invalid arguments/)
  })

  it('hides the counts when the option is turned off', async () => {
    const t = counting()
    await t.call('setExtensionActionOptions', { displayActionCountAsBadgeText: true })
    await req(t, 'https://ads.example/a.js')
    await t.call('setExtensionActionOptions', { displayActionCountAsBadgeText: false })
    expect(t.engine.actionCounts()).toEqual({})
    await req(t, 'https://ads.example/a.js')
    expect(t.engine.actionCounts()).toEqual({})
  })

  it('persists the option and reloads it', async () => {
    const t = counting()
    await t.call('setExtensionActionOptions', { displayActionCountAsBadgeText: true })
    t.store.flush()
    expect(new DnrStore(join(dir, 'dnr.json')).displayActionCount(ID)).toBe(true)
    await t.call('setExtensionActionOptions', { displayActionCountAsBadgeText: false })
    t.store.flush()
    expect(new DnrStore(join(dir, 'dnr.json')).displayActionCount(ID)).toBe(false)
  })

  it('pushes the counts at most every 250 ms', async () => {
    const t = counting()
    await t.call('setExtensionActionOptions', { displayActionCountAsBadgeText: true })
    t.pushed.length = 0
    vi.advanceTimersByTime(300)
    t.pushed.length = 0
    for (let i = 0; i < 50; i++) await req(t, `https://ads.example/${i}.js`)
    expect(t.pushed).toHaveLength(0)
    vi.advanceTimersByTime(249)
    expect(t.pushed).toHaveLength(0)
    vi.advanceTimersByTime(1)
    expect(t.pushed).toEqual([{ 1: { [ID]: 50 } }])
    vi.advanceTimersByTime(1000)
    expect(t.pushed).toHaveLength(1)
  })
})

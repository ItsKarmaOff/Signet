/* eslint-disable @typescript-eslint/no-explicit-any */
import { afterEach, describe, expect, it, vi } from 'vitest'
import { EXTENSION_API_CATALOG } from '@shared/extension-api-catalog'
import { installApis, type Plan } from './extension-apis-install'

const g = globalThis as unknown as Record<string, any>

const plan = (): Plan => ({
  settings: {},
  settingMethods: ['get', 'set', 'clear'],
  alarmEvent: 'alarms.onAlarm',
  proxyErrorEvent: 'proxy.onProxyError',
  namespacePermissions: { webRequest: ['webRequest'] },
  offscreenReasons: [],
  dnrEnums: {},
  dnrConstants: {},
  authEvent: 'webRequest.onAuthRequired',
  authResponseMethod: 'a',
  authListenersMethod: 'b',
  catalog: EXTENSION_API_CATALOG
})

/** Chrome's "Method not implemented" event, as the library defines it. */
const brokenEvent = () => ({
  addListener: () => {},
  removeListener: () => {},
  hasListener: () => {
    throw new Error('Method not implemented.')
  },
  hasListeners: () => {
    throw new Error('Method not implemented.')
  }
})

function install(manifest: Record<string, unknown>, extra: Record<string, unknown> = {}, opts: { frozen?: boolean } = {}) {
  const nativeAdd = vi.fn()
  const storageArea = (name: string) => ({
    name,
    get: () => {},
    onChanged: { addListener() {}, removeListener() {}, hasListener: () => false, hasListeners: () => false }
  })
  const local = storageArea('local')
  const chrome: Record<string, any> = {
    runtime: { getManifest: () => manifest, id: 'x' },
    tabs: { query: () => {} },
    storage: Object.freeze({ local, session: storageArea('session'), managed: local }),
    // Native-like namespace: a lazy getter that must never be read while copying.
    get system() {
      return Object.freeze({ cpu: { getInfo: () => {} }, storage: Object.freeze({ onAttached: { addListener: nativeAdd } }) })
    },
    ...extra
  }
  g.chrome = opts.frozen === false ? chrome : Object.freeze(chrome)
  g.browser = {}
  g.__signetCrx = { invoke: async () => ({ ok: true }), subscribe: () => {} }
  const report = installApis(plan())
  return { report, c: g.chrome, nativeAdd }
}

describe('chrome.permissions', () => {
  it('is installed for every extension, with the Signet members, and keeps optional namespaces for feature detection', () => {
    const { report, c } = install({ permissions: ['storage'], optional_permissions: ['proxy'] })
    expect(report.namespaces).toEqual(expect.arrayContaining(['permissions', 'proxy']))
    for (const m of ['request', 'contains', 'getAll', 'remove']) expect(typeof c.permissions[m]).toBe('function')
    for (const e of ['onAdded', 'onRemoved']) expect(typeof c.permissions[e].addListener).toBe('function')
    expect(c.privacy).toBeUndefined()
  })
})

describe('member overrides', () => {
  it('replaces single members of tabs, notifications, action and contextMenus and keeps the rest of the namespace', () => {
    const query = () => 'library'
    const { c } = install(
      { permissions: ['notifications', 'contextMenus'], action: {} },
      {
        tabs: { query },
        notifications: { create: () => 'library', getAll: () => {} },
        action: { setTitle: () => 'library', setIcon: () => 'library' },
        contextMenus: { create: () => 'library' }
      }
    )
    expect(c.tabs.query).toBe(query)
    for (const m of ['captureVisibleTab', 'highlight', 'insertCSS', 'removeCSS', 'detectLanguage']) expect(c.tabs[m]).toBeTypeOf('function')
    expect(c.notifications.onButtonClicked.addListener).toBeTypeOf('function')
    expect(c.action.setTitle()).toBe('library')
    for (const m of ['enable', 'disable', 'isEnabled', 'setBadgeTextColor', 'getBadgeTextColor']) expect(c.action[m]).toBeTypeOf('function')
    expect(c.contextMenus.update).toBeTypeOf('function')
    expect(c.contextMenus.create({ title: 'x', id: 'a' })).toBe('a')
  })

  it('leaves an absent namespace absent and sends non-imageData setIcon to the library', () => {
    const setIcon = vi.fn(() => 'library')
    const { c } = install({ permissions: [] }, { action: { setIcon } })
    expect(c.notifications).toBeUndefined()
    expect(c.action.setIcon({ path: 'a.png' })).toBe('library')
    expect(setIcon).toHaveBeenCalledWith({ path: 'a.png' })
  })
})

afterEach(() => {
  delete g.chrome
  delete g.browser
  delete g.__signetCrx
})

describe('installApis safety net', () => {
  it('fills the namespaces the manifest asks for, with constants', () => {
    const { c, report } = install({ permissions: ['tabGroups', 'userScripts'] })
    expect(c.tabGroups.TAB_GROUP_ID_NONE).toBe(-1)
    expect(c.tabs.TAB_ID_NONE).toBe(-1)
    expect(typeof c.userScripts.register).toBe('function')
    expect(report.stubbed).toContain('userScripts')
    expect(report.stubbed).toContain('tabs.group')
    expect(Object.isFrozen(c)).toBe(true)
    expect(g.browser).toBe(c)
  })

  it('does not install namespaces without permission or manifest key', () => {
    const { c } = install({ permissions: ['storage'] })
    expect(c.bookmarks).toBeUndefined()
    expect(c.sidePanel).toBeUndefined()
    expect(c.userScripts).toBeUndefined()
    const side = install({ side_panel: { default_path: 'p.html' } }).c
    expect(typeof side.sidePanel.open).toBe('function')
  })

  it('optional permissions unlock too', () => {
    const { c } = install({ permissions: [], optional_permissions: ['bookmarks'] })
    expect(typeof c.bookmarks.getTree).toBe('function')
  })

  it('never overrides an existing member', () => {
    const query = () => 'native'
    const { c } = install({ permissions: [] }, { tabs: { query } })
    expect(c.tabs.query).toBe(query)
    expect(typeof c.tabs.group).toBe('function')
  })

  it('method stub: callback style sets runtime.lastError, promise style rejects', async () => {
    const { c } = install({ permissions: ['userScripts'] })
    let seen: string | undefined
    c.userScripts.register([], () => {
      seen = c.runtime.lastError?.message
    })
    expect(seen).toBe('chrome.userScripts.register is not supported in Signet')
    expect(c.runtime.lastError).toBeUndefined()
    await expect(c.userScripts.register([])).rejects.toThrow('chrome.userScripts.register is not supported in Signet')
  })

  it('soft methods resolve a neutral value', async () => {
    const { c } = install({ permissions: ['topSites', 'sessions'] })
    await expect(c.topSites.get()).resolves.toEqual([])
    let got: unknown
    c.sessions.getRecentlyClosed((r: unknown) => (got = r))
    expect(got).toEqual([])
  })

  it('event stubs track listeners; declarative events resolve empty rules', async () => {
    const { c } = install({ permissions: ['tabGroups', 'declarativeContent'] })
    const fn = () => {}
    c.tabGroups.onCreated.addListener(fn)
    expect(c.tabGroups.onCreated.hasListener(fn)).toBe(true)
    c.tabGroups.onCreated.removeListener(fn)
    expect(c.tabGroups.onCreated.hasListeners()).toBe(false)
    await expect(c.declarativeContent.onPageChanged.getRules()).resolves.toEqual([])
    await expect(c.declarativeContent.onPageChanged.addRules([{}])).resolves.toEqual([])
    expect(new c.declarativeContent.PageStateMatcher({ pageUrl: 1 }).pageUrl).toBe(1)
  })

  it('works when chrome is not frozen (ran before the library)', () => {
    const { c, report } = install({ permissions: ['bookmarks'] }, {}, { frozen: false })
    expect(report.mode).toBe('first')
    expect(typeof c.bookmarks.getTree).toBe('function')
  })

  it('without a readable manifest, no stubs', () => {
    g.chrome = Object.freeze({
      runtime: {
        getManifest: () => {
          throw new Error('x')
        }
      }
    })
    g.__signetCrx = { invoke: async () => ({ ok: true }), subscribe: () => {} }
    const r = installApis(plan())
    expect(r.stubbed).toEqual([])
    expect(g.chrome.tabs).toBeUndefined()
  })
})

describe('native crashers', () => {
  it('replaces system.storage and printerProvider events with inert stubs', () => {
    const { c, nativeAdd } = install(
      { permissions: ['system.storage', 'system.cpu', 'printerProvider'] },
      { printerProvider: { onPrintRequested: { addListener: vi.fn() } } }
    )
    c.system.storage.onAttached.addListener(() => {})
    c.system.storage.onDetached.addListener(() => {})
    c.printerProvider.onPrintRequested.addListener(() => {})
    c.printerProvider.onGetUsbPrinterInfoRequested.addListener(() => {})
    expect(nativeAdd).not.toHaveBeenCalled()
    expect(c.system.storage.onAttached.hasListeners()).toBe(true)
  })

  it('system.storage.getInfo resolves an empty list and cpu stays native', async () => {
    const { c, report } = install({ permissions: ['system.storage', 'system.cpu'] })
    await expect(c.system.storage.getInfo()).resolves.toEqual([])
    expect(report.stubbed).toContain('system.storage.onAttached')
    expect(report.stubbed).not.toContain('system.cpu.getInfo')
  })

  it('does not touch them without the permission', () => {
    const { c, nativeAdd } = install({ permissions: [] })
    c.system.storage.onAttached.addListener(() => {})
    expect(nativeAdd).toHaveBeenCalledOnce()
  })
})

describe('storage.managed', () => {
  it('is an empty read-only area, not an alias of local', async () => {
    const { c } = install({ permissions: ['storage'] })
    expect(c.storage.managed).not.toBe(c.storage.local)
    await expect(c.storage.managed.get(null)).resolves.toEqual({})
    await expect(c.storage.managed.get({ a: 1 })).resolves.toEqual({ a: 1 })
    await expect(c.storage.managed.getBytesInUse()).resolves.toBe(0)
    await expect(c.storage.managed.set({ a: 1 })).rejects.toThrow('This is a read-only store.')
    await expect(c.storage.managed.remove('a')).rejects.toThrow('read-only')
    await expect(c.storage.managed.clear()).rejects.toThrow('read-only')
    let err: string | undefined
    c.storage.managed.set({}, () => (err = c.runtime.lastError?.message))
    expect(err).toBe('This is a read-only store.')
    expect(c.storage.managed.onChanged.hasListeners()).toBe(false)
    expect(c.storage.local.name).toBe('local')
  })

  it('is left alone without the storage permission', () => {
    const { c } = install({ permissions: [] })
    expect(c.storage.managed).toBe(c.storage.local)
  })
})

describe('library events', () => {
  it('repairs hasListener / hasListeners that throw', () => {
    const { c, report } = install({ permissions: [] }, { action: { onUserSettingsChanged: brokenEvent() } })
    const ev = c.action.onUserSettingsChanged
    const fn = () => {}
    expect(() => ev.hasListener(fn)).not.toThrow()
    ev.addListener(fn)
    expect(ev.hasListener(fn)).toBe(true)
    expect(ev.hasListeners()).toBe(true)
    ev.removeListener(fn)
    expect(ev.hasListeners()).toBe(false)
    expect(report.fixedEvents).toContain('action.onUserSettingsChanged')
  })

  it('leaves working events alone', () => {
    const own = { addListener: vi.fn(), removeListener: vi.fn(), hasListener: () => false, hasListeners: () => false }
    const { c, report } = install({ permissions: [] }, { windows: { onCreated: own } })
    expect(c.windows.onCreated.addListener).toBe(own.addListener)
    expect(report.fixedEvents).not.toContain('windows.onCreated')
  })
})

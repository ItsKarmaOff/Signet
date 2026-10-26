import { readFileSync } from 'node:fs'
import { join } from 'node:path'
import { beforeEach, describe, expect, it, vi } from 'vitest'

// Simulated Electron: `ipcMain` records the registered channels; everything else is an empty shell (modules load, nothing runs).
const ipc = vi.hoisted(() => ({
  handlers: new Map<string, (e: unknown, ...args: unknown[]) => unknown>(),
  listeners: new Map<string, (e: unknown, ...args: unknown[]) => unknown>(),
  registrations: [] as string[]
}))

vi.mock('electron', () => {
  const stub = (): unknown =>
    new Proxy(function () {}, {
      get: (_t, key) => (key === 'then' || key === Symbol.toPrimitive ? undefined : stub()),
      apply: () => stub(),
      construct: () => stub() as object
    })
  const ipcMain = {
    handle: (channel: string, fn: (e: unknown, ...args: unknown[]) => unknown) => {
      ipc.registrations.push(channel)
      ipc.handlers.set(channel, fn)
    },
    on: (channel: string, fn: (e: unknown, ...args: unknown[]) => unknown) => {
      ipc.registrations.push(channel)
      ipc.listeners.set(channel, fn)
    }
  }
  // One export per name imported from 'electron' in src/main (add one here if a new name appears).
  const names = ['BrowserWindow', 'Menu', 'MenuItem', 'WebContentsView', 'app', 'clipboard', 'desktopCapturer', 'dialog', 'globalShortcut']
  names.push('nativeImage', 'nativeTheme', 'net', 'powerMonitor', 'protocol', 'safeStorage', 'screen', 'session', 'shell', 'systemPreferences', 'webContents')
  return { ipcMain, ...Object.fromEntries(names.map((n) => [n, stub()])) }
})

import { IPC, PAGE_IPC, PIP_IPC } from '@shared/ipc/contract'
import type { Browser } from '../browser/browser'
import { registerIpc } from './index'

/** Simulated browser: every method exists and returns `undefined`; `tabIdOfContents` says whether the sender is a tab. */
function stubBrowser(tabContents: number[] = []): Browser {
  const deep = (): unknown => new Proxy(function () {}, { get: () => deep(), apply: () => undefined })
  return new Proxy({ views: { tabIdOfContents: (id: number) => (tabContents.includes(id) ? 'tab-1' : null) } } as object, {
    get: (t, key) => (key in t ? t[key as keyof typeof t] : deep())
  }) as unknown as Browser
}

const uiEvent = { senderFrame: { url: 'signetui://app/index.html#main' }, sender: { id: 1 } }

beforeEach(() => {
  ipc.handlers.clear()
  ipc.listeners.clear()
  ipc.registrations.length = 0
})

describe('IPC registration', () => {
  it('every contract call has exactly one handler, and nothing else is registered', () => {
    const b = stubBrowser()
    const notes = vi.fn()
    const passwords = vi.fn()
    ;(b as unknown as { notes: unknown }).notes = { registerIpc: notes }
    ;(b as unknown as { passwords: unknown }).passwords = { registerPageIpc: passwords }
    registerIpc(b, false)
    expect(notes).toHaveBeenCalledOnce()
    expect(passwords).toHaveBeenCalledOnce()
    const expected = [
      ...Object.values(IPC)
        .filter((d) => d.kind === 'invoke')
        .map((d) => d.channel),
      ...Object.values(PIP_IPC).map((d) => d.channel),
      // Web pages: swipe and forms (ipc/page.ts); notes and passwords register themselves (see below).
      ...Object.keys(PAGE_IPC).filter((c) => !/^(pw|notes):/.test(c))
    ]
    const dupes = ipc.registrations.filter((c, i) => ipc.registrations.indexOf(c) !== i)
    expect(dupes).toEqual([])
    const missing = expected.filter((c) => !ipc.registrations.includes(c))
    const extra = ipc.registrations.filter((c) => !expected.includes(c))
    expect(missing).toEqual([])
    expect(extra).toEqual([])
  })

  it('each notes and passwords page channel is handled by its module, and by it alone', () => {
    const dir = join(__dirname, '..')
    const sources = ['features/notes/notes.ts', 'features/passwords/passwords.ts'].map((f) => readFileSync(join(dir, f), 'utf8'))
    for (const channel of Object.keys(PAGE_IPC).filter((c) => /^(pw|notes):/.test(c))) {
      expect(
        sources.filter((src) => src.includes(`'${channel}'`)),
        channel
      ).toHaveLength(1)
    }
  })

  it('refuses a duplicate registration', () => {
    const reg = registerIpc(stubBrowser(), false)
    expect(() => reg.handle('getState', () => null as never)).toThrow(/already registered/)
  })

  it('refuses calls that do not come from the interface (web page, tab)', () => {
    registerIpc(stubBrowser([2]), false)
    const call = ipc.handlers.get('tab:activate')!
    expect(() => call({ senderFrame: { url: 'https://example.com/' }, sender: { id: 1 } }, 'a')).toThrow(/refused/)
    expect(() => call({ senderFrame: null, sender: { id: 1 } }, 'a')).toThrow(/refused/)
    // App page, but shown in a tab (note, Easel): refused too.
    expect(() => call({ senderFrame: { url: 'signetui://app/index.html#note/x' }, sender: { id: 2 } }, 'a')).toThrow(/refused/)
    expect(() => call(uiEvent, 'a')).not.toThrow()
  })

  it('checks arguments before the handler', () => {
    const b = stubBrowser()
    const activate = vi.fn()
    ;(b as unknown as { tabs: unknown }).tabs = { activate }
    registerIpc(b, false)
    const call = ipc.handlers.get('tab:activate')!
    expect(() => call(uiEvent, 42)).toThrow(/Invalid IPC call/)
    expect(() => call(uiEvent)).toThrow(/Invalid IPC call/)
    expect(activate).not.toHaveBeenCalled()
    call(uiEvent, 'tab-1', 'extra ignored')
    expect(activate).toHaveBeenCalledWith('tab-1')
  })

  it('malformed capture: { ok: false } instead of an error', () => {
    registerIpc(stubBrowser(), false)
    const call = ipc.handlers.get('capture:take')!
    expect(call(uiEvent, 't', { kind: 'region', rect: { x: NaN, y: 0, width: 1, height: 1 } }, 'copy')).toEqual({ ok: false })
    expect(call(uiEvent, 't', { kind: 'visible' }, 'print')).toEqual({ ok: false })
  })

  it('mini player sends ignore invalid or foreign calls', () => {
    const b = stubBrowser()
    const drag = vi.fn()
    ;(b as unknown as { pip: unknown }).pip = { ownsSender: () => true, drag }
    registerIpc(b, false)
    const send = ipc.listeners.get('pip:drag')!
    send(uiEvent, 'oui')
    send({ senderFrame: { url: 'https://example.com/' }, sender: { id: 1 } }, true)
    expect(drag).not.toHaveBeenCalled()
    send(uiEvent, true)
    expect(drag).toHaveBeenCalledWith(true)
  })
})

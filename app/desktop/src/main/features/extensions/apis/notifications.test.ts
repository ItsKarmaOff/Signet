import { describe, expect, it, vi } from 'vitest'
import {
  buttonIndex,
  buttonsOf,
  ExtensionNotifications,
  mergeOptions,
  toElectron,
  urgencyOf,
  type ChromeNotification,
  type ShownNotification
} from './notifications'
import type { ApiCaller } from './types'

const caller = { extensionId: 'ext' } as ApiCaller
const opts = (p: Partial<ChromeNotification> = {}): ChromeNotification => ({ type: 'basic', iconUrl: 'i.png', title: 'T', message: 'M', ...p })

describe('options mapping', () => {
  it('maps title, message, priority, silent and requireInteraction', () => {
    expect(toElectron(opts({ priority: 2, silent: true, requireInteraction: true, contextMessage: 'ctx' }), 'linux', 'Signet')).toEqual({
      title: 'T',
      subtitle: 'ctx',
      body: 'M',
      silent: true,
      urgency: 'critical',
      timeoutType: 'never'
    })
    expect(toElectron(opts(), 'linux', 'Signet', '/x/i.png')).toMatchObject({ subtitle: 'Signet', icon: '/x/i.png', urgency: 'normal', timeoutType: 'default' })
    expect(urgencyOf(-1)).toBe('low')
  })
  it('joins list items', () => {
    expect(
      toElectron(
        opts({
          type: 'list',
          items: [
            { title: 'a', message: '1' },
            { title: 'b', message: '2' }
          ]
        }),
        'linux',
        'S'
      ).body
    ).toBe('a - 1\nb - 2')
  })
  it('shows at most two buttons as macOS actions and none elsewhere', () => {
    const buttons = [{ title: 'Yes' }, { title: 'No' }, { title: 'Maybe' }]
    expect(toElectron(opts({ buttons }), 'darwin', 'S').actions).toEqual([
      { type: 'button', text: 'Yes' },
      { type: 'button', text: 'No' }
    ])
    expect(toElectron(opts({ buttons }), 'win32', 'S').actions).toBeUndefined()
    expect(buttonsOf({ buttons }, 'linux')).toEqual([])
  })
  it('maps an action index to the button index, refusing unknown ones', () => {
    const o = { buttons: [{ title: 'a' }, { title: 'b' }, { title: 'c' }] }
    expect(buttonIndex(1, o, 'darwin')).toBe(1)
    expect(buttonIndex(2, o, 'darwin')).toBeUndefined()
    expect(buttonIndex(0, o, 'win32')).toBeUndefined()
    expect(buttonIndex(-1, o, 'darwin')).toBeUndefined()
  })
  it('merges update options and never changes the type', () => {
    expect(mergeOptions(opts(), { title: 'N', type: 'list', message: undefined })).toMatchObject({ type: 'basic', title: 'N', message: 'M' })
  })
})

function setup(platform: 'darwin' | 'linux' = 'darwin') {
  const shown: (ShownNotification & { options: unknown; fire: (e: string, ...a: unknown[]) => void; closed: number })[] = []
  const emitter = { emit: vi.fn() }
  const n = new ExtensionNotifications({
    emitter,
    platform,
    appName: 'Signet',
    make: (options) => {
      const handlers = new Map<string, (...a: unknown[]) => void>()
      const s = {
        options,
        closed: 0,
        show: vi.fn(),
        close() {
          s.closed++
          handlers.get('close')?.()
        },
        on: (e: string, fn: (...a: unknown[]) => void) => handlers.set(e, fn),
        fire: (e: string, ...a: unknown[]) => handlers.get(e)?.(...a)
      }
      shown.push(s as never)
      return s as never
    },
    icon: (_c, url) => (url === 'bad.png' ? null : url),
    supported: () => true
  })
  return { n, shown, emitter }
}

describe('ExtensionNotifications', () => {
  it('create returns the id (generated when absent), shows it and lists it', () => {
    const { n, shown } = setup()
    expect(n.create(caller, 'a', opts())).toBe('a')
    const generated = n.create(caller, opts())
    expect(generated).toMatch(/^signet-notification-/)
    expect(shown).toHaveLength(2)
    expect(Object.keys(n.getAll(caller))).toEqual(['a', generated])
    expect(() => n.create(caller, 'b', { type: 'basic', title: 'x' } as ChromeNotification)).toThrow(/Missing/)
    expect(() => n.create(caller, 'b', opts({ iconUrl: 'bad.png' }))).toThrow(/images/)
  })

  it('sends click, button click (wakes the worker) and close events', () => {
    const { n, shown, emitter } = setup()
    n.create(caller, 'a', opts({ buttons: [{ title: 'Yes' }, { title: 'No' }] }))
    shown[0].fire('click')
    shown[0].fire('action', {}, 1)
    shown[0].fire('action', {}, 5)
    expect(emitter.emit).toHaveBeenCalledWith('ext', 'notifications.onClicked', ['a'], true)
    expect(emitter.emit).toHaveBeenCalledWith('ext', 'notifications.onButtonClicked', ['a', 1], true)
    expect(emitter.emit).toHaveBeenCalledTimes(2)
    shown[0].fire('close')
    expect(emitter.emit).toHaveBeenLastCalledWith('ext', 'notifications.onClosed', ['a', true], true)
    expect(n.getAll(caller)).toEqual({})
  })

  it('update shows the merged options again without a closed event, and is true only for an existing id', () => {
    const { n, shown, emitter } = setup()
    expect(n.update(caller, 'nope', { title: 'x' })).toBe(false)
    n.create(caller, 'a', opts())
    expect(n.update(caller, 'a', { title: 'New', message: 'Msg' })).toBe(true)
    expect(shown).toHaveLength(2)
    expect(shown[0].closed).toBe(1)
    expect(shown[1].options).toMatchObject({ title: 'New', body: 'Msg' })
    expect(emitter.emit).not.toHaveBeenCalled()
    shown[0].fire('click')
    expect(emitter.emit).not.toHaveBeenCalled()
    shown[1].fire('click')
    expect(emitter.emit).toHaveBeenCalledWith('ext', 'notifications.onClicked', ['a'], true)
  })

  it('clear reports a closure not made by the user; replacing an id is silent; unload closes quietly', () => {
    const { n, shown, emitter } = setup()
    n.create(caller, 'a', opts())
    expect(n.clear(caller, 'a')).toBe(true)
    expect(n.clear(caller, 'a')).toBe(false)
    expect(emitter.emit).toHaveBeenCalledWith('ext', 'notifications.onClosed', ['a', false], true)
    emitter.emit.mockClear()
    n.create(caller, 'b', opts())
    n.create(caller, 'b', opts({ title: 'again' }))
    n.unload('ext')
    expect(emitter.emit).not.toHaveBeenCalled()
    expect(shown.every((s) => s.closed === 1)).toBe(true)
  })
})

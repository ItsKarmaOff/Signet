import { EventEmitter } from 'node:events'
import { describe, expect, it, vi } from 'vitest'
import { errorString, isFragmentChange, NavigationExtras, type NavigationSource } from './navigation'

describe('isFragmentChange', () => {
  it('is true only when the fragment is the sole difference', () => {
    expect(isFragmentChange('https://a/p', 'https://a/p#x')).toBe(true)
    expect(isFragmentChange('https://a/p#x', 'https://a/p#y')).toBe(true)
    expect(isFragmentChange('https://a/p#x', 'https://a/p')).toBe(true)
    expect(isFragmentChange('https://a/p#x', 'https://a/p#x')).toBe(false)
    expect(isFragmentChange('https://a/p', 'https://a/q#x')).toBe(false)
    expect(isFragmentChange('https://a/p?x=1', 'https://a/p?x=2#y')).toBe(false)
    expect(isFragmentChange(undefined, 'https://a/p#x')).toBe(false)
    expect(isFragmentChange('nope', 'https://a/p#x')).toBe(false)
  })
})

describe('errorString', () => {
  it('writes Chrome net error names', () => {
    expect(errorString(-105, 'ERR_NAME_NOT_RESOLVED')).toBe('net::ERR_NAME_NOT_RESOLVED')
    expect(errorString(-105, 'net::ERR_X')).toBe('net::ERR_X')
    expect(errorString(-999, 'weird')).toBe('net::ERR_FAILED (-999)')
  })
})

function setup() {
  const wc = Object.assign(new EventEmitter(), { id: 4, getURL: () => 'https://a/p' }) as unknown as NavigationSource & EventEmitter
  const emitter = { emit: vi.fn(), extensionsWith: vi.fn(() => ['ext1', 'ext2']) }
  const nav = new NavigationExtras({
    emitter,
    frame: (pid, rid) =>
      rid === 9 ? undefined : { frameId: rid === 1 ? 0 : 5, parentFrameId: rid === 1 ? -1 : 0, frameType: rid === 1 ? 'outermost_frame' : 'sub_frame' },
    now: () => 123
  })
  nav.watch(wc)
  return { wc, emitter, nav }
}

describe('NavigationExtras', () => {
  it('sends onReferenceFragmentUpdated to extensions with webNavigation, without waking workers', () => {
    const { wc, emitter } = setup()
    wc.emit('did-navigate-in-page', {}, 'https://a/p#sec', true, 10, 1)
    expect(emitter.extensionsWith).toHaveBeenCalledWith('webNavigation')
    expect(emitter.emit).toHaveBeenCalledTimes(2)
    expect(emitter.emit).toHaveBeenCalledWith(
      'ext1',
      'webNavigation.onReferenceFragmentUpdated',
      [expect.objectContaining({ tabId: 4, frameId: 0, parentFrameId: -1, url: 'https://a/p#sec', timeStamp: 123, transitionType: 'link' })],
      false
    )
  })

  it('does not report a history API change (path changed) nor a repeat of the same fragment', () => {
    const { wc, emitter } = setup()
    wc.emit('did-navigate-in-page', {}, 'https://a/other', true, 10, 1)
    wc.emit('did-navigate-in-page', {}, 'https://a/other#a', true, 10, 1)
    wc.emit('did-navigate-in-page', {}, 'https://a/other#a', true, 10, 1)
    expect(emitter.emit).toHaveBeenCalledTimes(2)
  })

  it('a full navigation resets the reference URL', () => {
    const { wc, emitter } = setup()
    wc.emit('did-frame-navigate', {}, 'https://b/z', 200, 'OK', true, 10, 1)
    wc.emit('did-navigate-in-page', {}, 'https://b/z#k', true, 10, 1)
    expect(emitter.emit).toHaveBeenCalledWith('ext1', 'webNavigation.onReferenceFragmentUpdated', [expect.objectContaining({ url: 'https://b/z#k' })], false)
  })

  it('sends onErrorOccurred for failed loads, reports ERR_ABORTED once and skips frames that are gone', () => {
    const { wc, emitter } = setup()
    wc.emit('did-fail-load', {}, -105, 'ERR_NAME_NOT_RESOLVED', 'https://nope.test/', false, 10, 9)
    expect(emitter.emit).not.toHaveBeenCalled()
    wc.emit('did-fail-load', {}, -3, 'ERR_ABORTED', 'https://a/x', true, 10, 1)
    expect(emitter.emit).toHaveBeenCalledTimes(2)
    expect(emitter.emit).toHaveBeenCalledWith(
      'ext1',
      'webNavigation.onErrorOccurred',
      [expect.objectContaining({ error: 'net::ERR_ABORTED', frameId: 0 })],
      false
    )
    emitter.emit.mockClear()
    wc.emit('did-fail-load', {}, -105, 'ERR_NAME_NOT_RESOLVED', 'https://nope.test/', false, 10, 2)
    expect(emitter.emit).toHaveBeenCalledWith(
      'ext2',
      'webNavigation.onErrorOccurred',
      [expect.objectContaining({ error: 'net::ERR_NAME_NOT_RESOLVED', frameId: 5, parentFrameId: 0, frameType: 'sub_frame', url: 'https://nope.test/' })],
      false
    )
  })

  it('watches a tab once', () => {
    const { wc, emitter, nav } = setup()
    nav.watch(wc)
    wc.emit('did-fail-load', {}, -105, 'ERR_X', 'https://n/', true, 10, 1)
    expect(emitter.emit).toHaveBeenCalledTimes(2)
  })
})

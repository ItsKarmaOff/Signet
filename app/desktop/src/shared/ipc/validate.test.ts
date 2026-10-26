import { describe, expect, it } from 'vitest'
import { IPC } from './contract'
import {
  arr,
  bookmarkTarget,
  captureTarget,
  dragSource,
  dropTarget,
  firstInvalid,
  iconTarget,
  int,
  lit,
  nullable,
  num,
  obj,
  opt,
  paletteResult,
  popoverRequest,
  rect,
  spaceDraft,
  str
} from './validate'

describe('gardes de base', () => {
  it('str: string, optional maximum length', () => {
    expect(str()('')).toBe(true)
    expect(str()(1)).toBe(false)
    expect(str(3)('abc')).toBe(true)
    expect(str(3)('abcd')).toBe(false)
  })

  it('num : fini seulement ; int : entier', () => {
    expect(num()(1.5)).toBe(true)
    expect(num()(NaN)).toBe(false)
    expect(num()(Infinity)).toBe(false)
    expect(num()('1')).toBe(false)
    expect(int()(2)).toBe(true)
    expect(int()(2.5)).toBe(false)
  })

  it('lit, opt, nullable', () => {
    const l = lit('a', 'b')
    expect(l('a')).toBe(true)
    expect(l('c')).toBe(false)
    expect(opt(str())(undefined)).toBe(true)
    expect(opt(str())(null)).toBe(false)
    expect(nullable(str())(null)).toBe(true)
    expect(nullable(str())(undefined)).toBe(false)
  })

  it('arr: elements and size', () => {
    expect(arr(num())([1, 2])).toBe(true)
    expect(arr(num())([1, 'x'])).toBe(false)
    expect(arr(num())('12')).toBe(false)
    expect(arr(num(), 1)([1, 2])).toBe(false)
  })

  it('obj: known keys, optional, plain objects only', () => {
    const g = obj({ a: str(), b: opt(num()) })
    expect(g({ a: 'x' })).toBe(true)
    expect(g({ a: 'x', b: 1, extra: true })).toBe(true)
    expect(g({ a: 'x', b: 'y' })).toBe(false)
    expect(g({ b: 1 })).toBe(false)
    expect(g(null)).toBe(false)
    expect(g([])).toBe(false)
  })

  it('firstInvalid : index fautif, position absente = undefined', () => {
    expect(firstInvalid([str(), opt(num())], ['a'])).toBe(-1)
    expect(firstInvalid([str(), opt(num())], ['a', 'b'])).toBe(1)
    expect(firstInvalid([str()], [])).toBe(0)
  })
})

describe('gardes de domaine', () => {
  it('rect', () => {
    expect(rect({ x: 0, y: 1, width: 2, height: 3 })).toBe(true)
    expect(rect({ x: 0, y: 1, width: 2 })).toBe(false)
    expect(rect({ x: 0, y: 1, width: NaN, height: 3 })).toBe(false)
  })

  it('DragSource et DropTarget', () => {
    expect(dragSource({ kind: 'tab', tabId: 't' })).toBe(true)
    expect(dragSource({ kind: 'folder', id: 'f' })).toBe(true)
    expect(dragSource({ kind: 'tab', id: 'f' })).toBe(false)
    expect(dragSource({ kind: 'other', id: 'f' })).toBe(false)
    expect(dropTarget({ zone: 'pinned', spaceId: 's', index: 0 })).toBe(true)
    expect(dropTarget({ zone: 'pinned', spaceId: 's', folderId: 'f', index: 2 })).toBe(true)
    expect(dropTarget({ zone: 'nowhere', spaceId: 's', index: 0 })).toBe(false)
    expect(dropTarget({ zone: 'today', spaceId: 's' })).toBe(false)
  })

  it('CaptureTarget', () => {
    expect(captureTarget({ kind: 'visible' })).toBe(true)
    expect(captureTarget({ kind: 'region', rect: { x: 0, y: 0, width: 1, height: 1 } })).toBe(true)
    expect(captureTarget({ kind: 'region' })).toBe(false)
    expect(captureTarget({ kind: 'region', rect: { x: NaN, y: 0, width: 1, height: 1 } })).toBe(false)
  })

  it('SpaceDraft: plain object (main sanitizes)', () => {
    expect(spaceDraft({ name: 'a', icon: '✨', hue: 10 })).toBe(true)
    expect(spaceDraft('a')).toBe(false)
    expect(spaceDraft(null)).toBe(false)
  })

  it('BookmarkTarget, IconTarget', () => {
    expect(bookmarkTarget({ index: 0 })).toBe(true)
    expect(bookmarkTarget({ folderId: 'f', index: 1 })).toBe(true)
    expect(bookmarkTarget({ folderId: null, index: 1 })).toBe(false)
    expect(iconTarget({ kind: 'group', groupId: 'g' })).toBe(true)
    expect(iconTarget({ kind: 'group', folderId: 'g' })).toBe(false)
  })

  it('PopoverRequest et PaletteResult', () => {
    expect(popoverRequest({ kind: 'spaces' })).toBe(true)
    expect(popoverRequest({ kind: 'space-form', spaceId: null })).toBe(true)
    expect(popoverRequest({ kind: 'bookmarks', folderId: null, tabId: null })).toBe(true)
    expect(popoverRequest({ kind: 'folder' })).toBe(false)
    expect(paletteResult({ kind: 'url', id: '1', title: 't', url: 'https://a.b' })).toBe(true)
    expect(paletteResult({ kind: 'action', id: '1', title: 't', action: 'x', shortcut: '⌘T' })).toBe(true)
    expect(paletteResult({ kind: 'tab', id: '1', title: 't' })).toBe(false)
    expect(paletteResult({ kind: 'url', id: '1', title: 't' })).toBe(false)
    expect(paletteResult({ kind: 'tab', tabId: 't' })).toBe(true)
  })
})

describe('contract: real arguments', () => {
  const ok = (method: keyof typeof IPC, ...args: unknown[]) => {
    const def = IPC[method]
    return def.kind === 'invoke' && firstInvalid(def.args, args) === -1
  }

  it('accepts the calls the UI makes (optionals absent, null)', () => {
    expect(ok('goBack')).toBe(true)
    expect(ok('goBack', 'tab')).toBe(true)
    expect(ok('toggleFolder', 'f')).toBe(true)
    expect(ok('toggleFolder', 'f', false)).toBe(true)
    expect(ok('closeOverlay')).toBe(true)
    expect(ok('openPalette', 'new-tab')).toBe(true)
    expect(ok('openPalette', 'edit-url', { x: 1, y: 2, width: 3, height: 4 })).toBe(true)
    expect(ok('toggleBookmark', null)).toBe(true)
    expect(ok('setResponsive', 't', null)).toBe(true)
    expect(ok('resetSitePermissions')).toBe(true)
    expect(ok('pipCommand', 'seek', 12)).toBe(true)
    expect(ok('openLibrary')).toBe(true)
    expect(ok('setSidebarWidth', 240)).toBe(true)
  })

  it('rejects wrong types and missing arguments', () => {
    expect(ok('activateTab')).toBe(false)
    expect(ok('activateTab', 3)).toBe(false)
    expect(ok('moveTabStep', 't', 2)).toBe(false)
    expect(ok('setSidebarWidth', '240')).toBe(false)
    expect(ok('exportBackup', ['nope'], null)).toBe(false)
    expect(ok('exportBackup', ['settings', 'passwords'], 'secret')).toBe(true)
    expect(ok('answerPasswordPrompt', 't', 'maybe')).toBe(false)
    expect(ok('updateSettings', 'x')).toBe(false)
    expect(ok('updateSettings', { adblock: true })).toBe(true)
  })
})

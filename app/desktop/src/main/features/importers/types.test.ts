import { describe, expect, it } from 'vitest'
import { countTabs, isWebUrl, pruneEmpty, type ImportedNode } from './types'

const tab = (url: string): ImportedNode => ({ kind: 'tab', url, title: url })

describe('isWebUrl', () => {
  it('http(s) only', () => {
    expect(isWebUrl('https://a.com')).toBe(true)
    expect(isWebUrl('  HTTP://a.com')).toBe(true)
    expect(isWebUrl('javascript:1')).toBe(false)
    expect(isWebUrl('file:///x')).toBe(false)
    expect(isWebUrl(5)).toBe(false)
    expect(isWebUrl(undefined)).toBe(false)
  })
})

describe('pruneEmpty / countTabs', () => {
  const tree: ImportedNode[] = [
    tab('https://a.com'),
    { kind: 'folder', name: 'vide', items: [{ kind: 'folder', name: 'aussi vide', items: [] }] },
    { kind: 'folder', name: 'plein', items: [tab('https://b.com'), { kind: 'folder', name: 'x', items: [] }] },
    {
      kind: 'split',
      tabs: [
        { url: 'https://c.com', title: 'c' },
        { url: 'https://d.com', title: 'd' }
      ]
    }
  ]
  it('recursively removes empty folders', () => {
    const out = pruneEmpty(tree)
    expect(out.map((n) => n.kind)).toEqual(['tab', 'folder', 'split'])
    expect((out[1] as Extract<ImportedNode, { kind: 'folder' }>).items).toHaveLength(1)
  })
  it('counts tabs, split views included', () => {
    expect(countTabs(tree)).toBe(4)
    expect(countTabs(pruneEmpty(tree))).toBe(4)
    expect(countTabs([])).toBe(0)
  })
})

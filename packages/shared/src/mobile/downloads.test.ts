import { describe, expect, it } from 'vitest'
import {
  addDownload,
  clearDownloads,
  downloadKind,
  fileNameFromUrl,
  formatBytes,
  parseDownloads,
  removeDownload,
  sanitizeFileName,
  serializeDownloads,
  uniqueFileName,
  updateDownload,
  type DownloadEntry
} from './downloads'

const entry = (id: string, over: Partial<DownloadEntry> = {}): DownloadEntry => ({
  id,
  name: `${id}.pdf`,
  url: `https://a.test/${id}.pdf`,
  size: 10,
  date: 1,
  status: 'done',
  path: `file:///d/${id}.pdf`,
  ...over
})

describe('downloads file names', () => {
  it('derives names from urls', () => {
    expect(fileNameFromUrl('https://a.test/files/My%20Report.pdf?x=1')).toBe('My Report.pdf')
    expect(fileNameFromUrl('https://a.test/')).toBe('download')
    expect(fileNameFromUrl('nope')).toBe('download')
    expect(fileNameFromUrl('https://a.test/%E0%A4%A')).toBe('%E0%A4%A')
  })

  it('sanitizes', () => {
    expect(sanitizeFileName('../a/b:c?.txt')).toBe('_a_b_c_.txt')
    expect(sanitizeFileName('...')).toBe('download')
    const long = sanitizeFileName('x'.repeat(300) + '.pdf')
    expect(long).toHaveLength(120)
    expect(long.endsWith('.pdf')).toBe(true)
  })

  it('makes names unique, ignoring case', () => {
    expect(uniqueFileName('a.pdf', [])).toBe('a.pdf')
    expect(uniqueFileName('a.pdf', ['A.PDF', 'a (1).pdf'])).toBe('a (2).pdf')
    expect(uniqueFileName('noext', ['noext'])).toBe('noext (1)')
  })

  it('classifies and formats', () => {
    expect(downloadKind('Photo.JPG')).toBe('image')
    expect(downloadKind('a.zip')).toBe('archive')
    expect(downloadKind('a.unknown')).toBe('other')
    expect(downloadKind('noext')).toBe('other')
    expect(formatBytes(0)).toBe('')
    expect(formatBytes(512)).toBe('512 B')
    expect(formatBytes(1_400_000, ',')).toBe('1,4 MB')
    expect(formatBytes(150_000_000)).toBe('150 MB')
  })
})

describe('downloads list', () => {
  it('adds newest first, updates, removes and clears', () => {
    let list = addDownload([], entry('a'))
    list = addDownload(list, entry('b', { status: 'downloading', path: '' }))
    expect(list.map((d) => d.id)).toEqual(['b', 'a'])
    list = updateDownload(list, 'b', { status: 'done', progress: 1 })
    expect(list[0].status).toBe('done')
    expect(updateDownload(list, 'zzz', { size: 1 })).toBe(list)
    expect(removeDownload(list, 'a').map((d) => d.id)).toEqual(['b'])
    const running = addDownload(list, entry('c', { status: 'downloading', path: '' }))
    expect(clearDownloads(running).map((d) => d.id)).toEqual(['c'])
  })

  it('round-trips without progress and heals bad data', () => {
    const text = serializeDownloads([entry('a', { progress: 0.5 })])
    expect(text).not.toContain('progress')
    expect(parseDownloads(JSON.parse(text))).toEqual([entry('a')])
    const healed = parseDownloads([entry('a'), entry('a'), { id: 'x' }, null, entry('r', { status: 'downloading' }), entry('n', { path: '' })])
    expect(healed.map((d) => [d.id, d.status])).toEqual([
      ['a', 'done'],
      ['r', 'failed'],
      ['n', 'failed']
    ])
    expect(parseDownloads('nope')).toEqual([])
  })
})

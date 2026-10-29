import { describe, expect, it } from 'vitest'
import { bareUrl, canComplete, inlineCompletion } from './inline'

const page = (url: string, kind = 'history', inline = true) => ({ kind, url, inline })

describe('bareUrl', () => {
  it('drops scheme, www and trailing slash', () => {
    expect(bareUrl('https://www.youtube.com/')).toBe('youtube.com')
    expect(bareUrl('http://localhost:3000/a/')).toBe('localhost:3000/a')
  })
})

describe('inlineCompletion', () => {
  const yt = page('https://www.youtube.com/')
  it('completes the host and keeps the typed case', () => {
    expect(inlineCompletion('yout', yt)).toBe('ube.com')
    expect(inlineCompletion('YouTube', yt)).toBe('.com')
  })
  it('completes the path when typed beyond the host', () => {
    expect(inlineCompletion('youtube.com/fe', page('https://www.youtube.com/feed/subscriptions'))).toBe('ed/subscriptions')
  })
  it('only completes the host when no slash is typed', () => {
    expect(inlineCompletion('youtube', page('https://www.youtube.com/watch?v=1'))).toBe('.com')
  })
  it('works for tabs and archives, not for other rows', () => {
    expect(inlineCompletion('yo', page('https://youtube.com', 'tab'))).toBe('utube.com')
    expect(inlineCompletion('yo', page('https://youtube.com', 'archived'))).toBe('utube.com')
    for (const kind of ['search', 'suggestion', 'action', 'url']) expect(inlineCompletion('yo', page('https://youtube.com', kind))).toBe('')
  })
  it('nothing when the row is not flagged inline (never typed, rarely visited)', () => {
    expect(inlineCompletion('yout', page('https://www.youtube.com/', 'history', false))).toBe('')
    expect(inlineCompletion('yout', { kind: 'history', url: 'https://www.youtube.com/' })).toBe('')
  })
  it('nothing when not a prefix, complete, empty, spaced or not web', () => {
    expect(inlineCompletion('tube', yt)).toBe('')
    expect(inlineCompletion('youtube.com', yt)).toBe('')
    expect(inlineCompletion('', yt)).toBe('')
    expect(inlineCompletion('you tube', yt)).toBe('')
    expect(inlineCompletion('sett', page('signet://settings'))).toBe('')
    expect(inlineCompletion('yo', undefined)).toBe('')
  })
})

describe('canComplete', () => {
  it('only when a character is added at the end', () => {
    expect(canComplete('you', 'yout', 'insertText', false)).toBe(true)
    expect(canComplete('yout', 'you', 'deleteContentBackward', false)).toBe(false)
    expect(canComplete('you', 'yo', undefined, false)).toBe(false)
    expect(canComplete('you', 'yxou', 'insertText', false)).toBe(false)
  })
  it('not while composing or on paste', () => {
    expect(canComplete('you', 'yout', 'insertText', true)).toBe(false)
    expect(canComplete('you', 'youtube', 'insertFromPaste', false)).toBe(false)
  })
})

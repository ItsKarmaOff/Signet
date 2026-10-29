import { describe, expect, it } from 'vitest'
import { backgroundContext, filterContexts, pageContext } from './contexts'

const A = 'a'.repeat(32)
const worker = backgroundContext(A, `chrome-extension://${A}/sw.js`)
const popup = pageContext(A, 5, `chrome-extension://${A}/popup.html`, 'POPUP')

describe('filterContexts', () => {
  it('keeps everything without a filter', () => {
    expect(filterContexts([worker, popup], undefined)).toHaveLength(2)
    expect(filterContexts([worker, popup], {})).toHaveLength(2)
  })
  it('matches every given property', () => {
    expect(filterContexts([worker, popup], { contextTypes: ['BACKGROUND'] })).toEqual([worker])
    expect(filterContexts([worker, popup], { contextTypes: ['POPUP', 'TAB'], documentUrls: [popup.documentUrl] })).toEqual([popup])
    expect(filterContexts([worker, popup], { contextTypes: ['OFFSCREEN_DOCUMENT'] })).toEqual([])
    expect(filterContexts([worker, popup], { incognito: true })).toEqual([])
    expect(filterContexts([worker, popup], { tabIds: [3] })).toEqual([])
  })
})

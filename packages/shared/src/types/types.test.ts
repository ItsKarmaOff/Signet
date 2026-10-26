import { describe, expect, it } from 'vitest'
import { DEFAULT_READER, HISTORY_QUERY_MAX } from './index'

describe('data model constants', () => {
  it('reading: default values', () => {
    expect(DEFAULT_READER).toEqual({ font: 'serif', size: 19, theme: 'auto', width: 'normal' })
    expect(DEFAULT_READER.size).toBeGreaterThanOrEqual(14)
    expect(DEFAULT_READER.size).toBeLessThanOrEqual(28)
  })

  it('history limit', () => {
    expect(HISTORY_QUERY_MAX).toBe(500)
  })
})

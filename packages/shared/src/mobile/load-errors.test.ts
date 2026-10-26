import { describe, expect, it } from 'vitest'
import { classifyLoadError, isCancelledLoad } from './load-errors'

describe('classifyLoadError', () => {
  it('iOS', () => {
    expect(classifyLoadError(-1009, 'The Internet connection appears to be offline.')).toBe('offline')
    expect(classifyLoadError(-1003, 'A server with the specified hostname could not be found.')).toBe('notFound')
    expect(classifyLoadError(-1001, 'The request timed out.')).toBe('timeout')
    expect(classifyLoadError(-1202, 'The certificate for this server is invalid.')).toBe('secure')
    expect(classifyLoadError(-1004, 'Could not connect to the server.')).toBe('refused')
  })
  it('Android', () => {
    expect(classifyLoadError(-2, 'net::ERR_NAME_NOT_RESOLVED')).toBe('notFound')
    expect(classifyLoadError(-2, 'net::ERR_INTERNET_DISCONNECTED')).toBe('offline')
    expect(classifyLoadError(-6, 'net::ERR_CONNECTION_REFUSED')).toBe('refused')
    expect(classifyLoadError(-7, 'net::ERR_TIMED_OUT')).toBe('timeout')
  })
  it('le reste', () => {
    expect(classifyLoadError(-1, 'weird')).toBe('other')
    expect(isCancelledLoad(-999)).toBe(true)
    expect(isCancelledLoad(-1009)).toBe(false)
  })
})

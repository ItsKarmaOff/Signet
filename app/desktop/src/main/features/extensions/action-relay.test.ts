import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { ACK_TIMEOUT_MS, ActionRelay } from './action-relay'

describe('ActionRelay', () => {
  beforeEach(() => vi.useFakeTimers())
  afterEach(() => vi.useRealTimers())

  const make = (delivered = true) => {
    const sent: [string, string][] = []
    const relay = new ActionRelay({
      send: (token, id) => {
        sent.push([token, id])
        return delivered
      }
    })
    return { relay, sent }
  }

  it('does nothing else when the toolbar acknowledges in time', () => {
    const { relay, sent } = make()
    const fallback = vi.fn()
    relay.request('ext', fallback)
    expect(sent).toHaveLength(1)
    expect(sent[0][1]).toBe('ext')
    expect(relay.ack(sent[0][0])).toBe(true)
    vi.advanceTimersByTime(ACK_TIMEOUT_MS * 2)
    expect(fallback).not.toHaveBeenCalled()
  })

  it('falls back after the timeout, and a late acknowledgement is refused', () => {
    const { relay, sent } = make()
    const fallback = vi.fn()
    relay.request('ext', fallback)
    vi.advanceTimersByTime(ACK_TIMEOUT_MS - 1)
    expect(fallback).not.toHaveBeenCalled()
    vi.advanceTimersByTime(1)
    expect(fallback).toHaveBeenCalledTimes(1)
    expect(relay.ack(sent[0][0])).toBe(false)
  })

  it('falls back at once when there is no toolbar to ask', () => {
    const { relay } = make(false)
    const fallback = vi.fn()
    relay.request('ext', fallback)
    expect(fallback).toHaveBeenCalledTimes(1)
  })

  it('only the first acknowledgement of a token counts, and tokens are independent', () => {
    const { relay, sent } = make()
    const a = vi.fn()
    const b = vi.fn()
    relay.request('a', a)
    relay.request('b', b)
    expect(relay.ack(sent[0][0])).toBe(true)
    expect(relay.ack(sent[0][0])).toBe(false)
    expect(relay.ack('unknown')).toBe(false)
    vi.advanceTimersByTime(ACK_TIMEOUT_MS)
    expect(a).not.toHaveBeenCalled()
    expect(b).toHaveBeenCalledTimes(1)
  })
})

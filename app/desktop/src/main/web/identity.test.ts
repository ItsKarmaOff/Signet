import { describe, expect, it, vi } from 'vitest'

vi.mock('electron', () => ({ app: { userAgentFallback: '', getPath: () => '' } }))

import { reducedUserAgent } from './identity'

describe('reducedUserAgent', () => {
  it('strips Electron and the app name, reduces the Chrome version', () => {
    const ua =
      'Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) signet/0.1.0 Chrome/156.0.8078.48 Electron/42.0.0 Safari/537.36'
    expect(reducedUserAgent(ua)).toBe('Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/156.0.0.0 Safari/537.36')
  })
  it('leaves an already-reduced agent as is', () => {
    expect(reducedUserAgent('X Chrome/120.0.0.0 Safari/537.36')).toBe('X Chrome/120.0.0.0 Safari/537.36')
  })
})

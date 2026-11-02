import { describe, expect, it } from 'vitest'
import { devUiUrl } from './dev-url'

describe('devUiUrl', () => {
  it('moves the Vite server off localhost, keeping port and path', () => {
    expect(devUiUrl('http://localhost:5173')).toBe('http://signet-ui.localhost:5173')
    expect(devUiUrl('http://localhost:5173/')).toBe('http://signet-ui.localhost:5173/')
    expect(devUiUrl('http://127.0.0.1:5174/index.html')).toBe('http://signet-ui.localhost:5174/index.html')
  })

  it('leaves other addresses alone', () => {
    expect(devUiUrl('http://example.test:5173')).toBe('http://example.test:5173')
    expect(devUiUrl('not a url')).toBe('not a url')
  })
})

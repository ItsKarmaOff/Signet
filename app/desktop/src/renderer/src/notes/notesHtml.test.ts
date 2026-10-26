import { describe, expect, it } from 'vitest'
import { docIcon } from './notesHtml'

// cleanHtml / firstLine / setTabLook need a DOM (template, document): no DOM environment installed, see the report.
describe('docIcon', () => {
  const decode = (url: string) => decodeURIComponent(url.replace('data:image/svg+xml,', ''))

  it('renvoie une data URL SVG', () => {
    expect(docIcon('note', 200)).toMatch(/^data:image\/svg\+xml,%3Csvg/)
  })

  it('colors the background with the given hue', () => {
    expect(decode(docIcon('note', 123))).toContain('fill="hsl(123 60% 52%)"')
  })

  it('draws a different glyph for a note and an Easel', () => {
    const note = decode(docIcon('note', 0))
    const easel = decode(docIcon('easel', 0))
    expect(note).toContain('M9 7h14')
    expect(note).not.toContain('<circle')
    expect(easel).toContain('<circle')
    expect(easel).not.toContain('M9 7h14')
  })

  it('is well-formed SVG (opened and closed tags)', () => {
    const svg = decode(docIcon('easel', 10))
    expect(svg.startsWith('<svg')).toBe(true)
    expect(svg.endsWith('</svg>')).toBe(true)
  })
})

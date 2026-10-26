import { describe, expect, it } from 'vitest'
import { sanitizeModCss } from './modscss'

const EMPTY = 'data:,'

describe('sanitizeModCss', () => {
  it('lets CSS without remote loading through', () => {
    const css = '.a { color: red; background: linear-gradient(#fff, #000) }'
    expect(sanitizeModCss(css)).toBe(css)
  })

  it('neutralizes remote url(), with or without quotes', () => {
    expect(sanitizeModCss('a{background:url(https://evil.com/x)}')).toBe(`a{background:url("${EMPTY}")}`)
    expect(sanitizeModCss('a{background:url("https://evil.com/x")}')).toBe(`a{background:url("${EMPTY}")}`)
    expect(sanitizeModCss("a{background:url( 'https://evil.com' )}")).toBe(`a{background:url("${EMPTY}")}`)
    expect(sanitizeModCss('a{background:url(//evil.com/x)}')).toBe(`a{background:url("${EMPTY}")}`)
  })

  it('keeps data: and local #id references', () => {
    expect(sanitizeModCss('a{background:url(data:image/png;base64,AAAA)}')).toBe('a{background:url(data:image/png;base64,AAAA)}')
    expect(sanitizeModCss('a{filter:url(#blur)}')).toBe('a{filter:url(#blur)}')
  })

  it('ignores case and CSS escapes', () => {
    expect(sanitizeModCss('a{background:URL(https://evil.com)}')).toContain(`url("${EMPTY}")`)
    expect(sanitizeModCss('a{background:\\75rl(https://evil.com)}')).toContain(`url("${EMPTY}")`)
    expect(sanitizeModCss('a{background:url(\\68ttps://evil.com)}')).toContain(`url("${EMPTY}")`)
  })

  it('spaces and line breaks before the data: scheme', () => {
    expect(sanitizeModCss('a{background:url(" \\n data:,x")}')).toContain('data:')
    expect(sanitizeModCss('a{background:url("  https://evil.com")}')).toContain(`url("${EMPTY}")`)
  })

  it('neutralizes @import (string or url)', () => {
    expect(sanitizeModCss('@import "https://evil.com/a.css";')).toBe(`@import "${EMPTY}";`)
    expect(sanitizeModCss('@import url(https://evil.com/a.css);')).toBe(`@import url("${EMPTY}");`)
    expect(sanitizeModCss('@\\69mport "https://evil.com";')).toContain(EMPTY)
  })

  it('neutralizes direct strings of image-set() and the like', () => {
    expect(sanitizeModCss('a{background:image-set("https://evil.com/a.png" 1x)}')).toBe(`a{background:image-set("${EMPTY}" 1x)}`)
    expect(sanitizeModCss('a{background:-webkit-image-set("https://evil.com/a.png" 1x)}')).toContain(EMPTY)
  })

  it('does not touch ordinary strings (content, selectors)', () => {
    const css = 'a::after{content:"https://example.com"} input[value^="https://x"]{color:red}'
    expect(sanitizeModCss(css)).toBe(css)
  })

  it('a comment does not hide a load', () => {
    // The comment splits the token: `u rl(` is not a url() for the browser.
    expect(sanitizeModCss('a{background:u/**/rl(https://evil.com)}')).toBe('a{background:u rl(https://evil.com)}')
  })

  it('unclosed string or url(): does not crash and leaks nothing', () => {
    expect(sanitizeModCss('a{background:url(https://evil.com')).not.toContain('evil.com')
    expect(sanitizeModCss('a{background:url("https://evil.com')).not.toContain('evil.com')
  })
})

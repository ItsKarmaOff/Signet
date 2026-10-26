// Neutralizes remote loads in a mod sheet: without this, `input[value^=a]{background:url(https://…)}` leaks
// UI data (the UI's CSP allows `img-src *` for favicons).
// Only `data:` and local references (`#id`) remain. CSS escapes, comments and case are taken into account.

/** Harmless replacement: a valid URL that loads nothing. */
const EMPTY = 'data:,'
/** Functions whose direct strings are images (`image-set("a.png" 1x)`). */
const IMAGE_FNS = new Set(['image-set', '-webkit-image-set', 'cross-fade', '-webkit-cross-fade', 'image', 'src'])

const isHex = (c: string): boolean => /^[0-9a-fA-F]$/.test(c)
const isWs = (c: string): boolean => c === ' ' || c === '\t' || c === '\n' || c === '\r' || c === '\f'
const isIdent = (c: string): boolean => /^[A-Za-z0-9_\-\u0080-￿]$/.test(c)

/** Reads an escape starting at `\` (position i); returns the decoded character and the next position. */
function escapeAt(s: string, i: number): { ch: string; next: number } {
  let j = i + 1
  if (j >= s.length) return { ch: '�', next: j }
  if (isHex(s[j])) {
    let hex = ''
    while (j < s.length && hex.length < 6 && isHex(s[j])) hex += s[j++]
    if (isWs(s[j] ?? '')) j += s[j] === '\r' && s[j + 1] === '\n' ? 2 : 1
    const cp = parseInt(hex, 16)
    return { ch: cp === 0 || cp > 0x10ffff || (cp >= 0xd800 && cp <= 0xdfff) ? '�' : String.fromCodePoint(cp), next: j }
  }
  const cp = s.codePointAt(j) as number
  const ch = String.fromCodePoint(cp)
  return { ch, next: j + ch.length }
}

/** Reads a string (quote at i); returns its decoded value and the position after the closing quote. */
function readString(s: string, i: number): { value: string; next: number } {
  const q = s[i]
  let j = i + 1
  let value = ''
  while (j < s.length) {
    const c = s[j]
    if (c === q) return { value, next: j + 1 }
    if (c === '\n' || c === '\r' || c === '\f') return { value, next: j } // invalid string: stops at the end of the line
    if (c === '\\') {
      if (s[j + 1] === '\n' || s[j + 1] === '\f') j += 2
      else if (s[j + 1] === '\r') j += s[j + 2] === '\n' ? 3 : 2
      else {
        const e = escapeAt(s, j)
        value += e.ch
        j = e.next
      }
      continue
    }
    value += c
    j++
  }
  return { value, next: j }
}

const allowed = (value: string): boolean => {
  // Control characters and leading spaces are ignored by the browser before reading the scheme.
  // eslint-disable-next-line no-control-regex
  const v = value.replace(/^[\u0000- ]+/, '').replace(/[\t\n\r]/g, '')
  return /^data:/i.test(v) || v.startsWith('#')
}

export function sanitizeModCss(css: string): string {
  let out = ''
  let i = 0
  const stack: boolean[] = [] // one entry per open parenthesis: true if its direct strings are images
  let importPending = false
  while (i < css.length) {
    const c = css[i]
    if (c === '/' && css[i + 1] === '*') {
      const end = css.indexOf('*/', i + 2)
      i = end < 0 ? css.length : end + 2
      out += ' '
    } else if (c === '"' || c === "'") {
      const s = readString(css, i)
      const watched = importPending || stack[stack.length - 1] === true
      importPending = false
      out += watched && !allowed(s.value) ? `"${EMPTY}"` : css.slice(i, s.next)
      i = s.next
    } else if (c === '(') {
      stack.push(false)
      out += c
      i++
    } else if (c === ')') {
      stack.pop()
      out += c
      i++
    } else if (c === ';' || c === '{') {
      importPending = false
      out += c
      i++
    } else if (c === '\\' || isIdent(c) || (c === '@' && (css[i + 1] === '\\' || isIdent(css[i + 1] ?? ' ')))) {
      const start = i
      const at = c === '@'
      if (at) i++
      let name = ''
      while (i < css.length) {
        if (css[i] === '\\') {
          const e = escapeAt(css, i)
          name += e.ch
          i = e.next
        } else if (isIdent(css[i])) name += css[i++]
        else break
      }
      const lower = name.toLowerCase()
      if (at) {
        if (lower === 'import') importPending = true
        out += css.slice(start, i)
      } else if (css[i] === '(' && lower === 'url') {
        // url( … ): string or raw value, up to the closing parenthesis.
        let j = i + 1
        while (j < css.length && isWs(css[j])) j++
        let value = ''
        if (css[j] === '"' || css[j] === "'") {
          const s = readString(css, j)
          value = s.value
          j = s.next
          while (j < css.length && css[j] !== ')') j++
        } else {
          while (j < css.length && css[j] !== ')') {
            if (css[j] === '\\') {
              const e = escapeAt(css, j)
              value += e.ch
              j = e.next
            } else value += css[j++]
          }
        }
        const end = Math.min(j + 1, css.length)
        out += allowed(value.trim()) ? css.slice(start, end) : `url("${EMPTY}")`
        i = end
      } else {
        out += css.slice(start, i)
        if (css[i] === '(') {
          stack.push(IMAGE_FNS.has(lower))
          out += '('
          i++
        }
      }
    } else {
      out += c
      i++
    }
  }
  return out
}

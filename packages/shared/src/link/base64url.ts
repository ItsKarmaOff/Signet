// base64url (RFC 4648 §5, without `=`) and UTF-8, without Buffer or btoa: this code also runs in Hermes.

const ALPHABET = 'ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_'
const LOOKUP: Record<string, number> = Object.fromEntries([...ALPHABET].map((c, i) => [c, i]))

export function base64UrlEncode(bytes: Uint8Array): string {
  let out = ''
  for (let i = 0; i < bytes.length; i += 3) {
    const n = (bytes[i]! << 16) | ((bytes[i + 1] ?? 0) << 8) | (bytes[i + 2] ?? 0)
    out += ALPHABET[(n >> 18) & 63]! + ALPHABET[(n >> 12) & 63]!
    if (i + 1 < bytes.length) out += ALPHABET[(n >> 6) & 63]!
    if (i + 2 < bytes.length) out += ALPHABET[n & 63]!
  }
  return out
}

/** Throws if the text is not valid base64url. */
export function base64UrlDecode(text: string): Uint8Array {
  if (text.length % 4 === 1) throw new Error('base64url: longueur invalide')
  const out: number[] = []
  let buf = 0
  let bits = 0
  for (const c of text) {
    const v = LOOKUP[c]
    if (v === undefined) throw new Error('base64url: invalid character')
    buf = (buf << 6) | v
    bits += 6
    if (bits >= 8) {
      bits -= 8
      out.push((buf >> bits) & 0xff)
    }
  }
  return Uint8Array.from(out)
}

export function utf8Encode(text: string): Uint8Array {
  const out: number[] = []
  for (const ch of text) {
    const cp = ch.codePointAt(0)!
    if (cp < 0x80) out.push(cp)
    else if (cp < 0x800) out.push(0xc0 | (cp >> 6), 0x80 | (cp & 63))
    else if (cp < 0x10000) out.push(0xe0 | (cp >> 12), 0x80 | ((cp >> 6) & 63), 0x80 | (cp & 63))
    else out.push(0xf0 | (cp >> 18), 0x80 | ((cp >> 12) & 63), 0x80 | ((cp >> 6) & 63), 0x80 | (cp & 63))
  }
  return Uint8Array.from(out)
}

/** Throws on an invalid UTF-8 sequence. */
export function utf8Decode(bytes: Uint8Array): string {
  let out = ''
  for (let i = 0; i < bytes.length;) {
    const b = bytes[i]!
    const n = b < 0x80 ? 0 : b >> 5 === 6 ? 1 : b >> 4 === 14 ? 2 : b >> 3 === 30 ? 3 : -1
    if (n < 0 || i + n >= bytes.length) throw new Error('utf8: invalid sequence')
    let cp = n === 0 ? b : b & (0x3f >> n)
    for (let k = 1; k <= n; k++) {
      const c = bytes[i + k]
      if (c === undefined || c >> 6 !== 2) throw new Error('utf8: invalid sequence')
      cp = (cp << 6) | (c & 63)
    }
    out += String.fromCodePoint(cp)
    i += n + 1
  }
  return out
}

export const base64UrlEncodeText = (text: string): string => base64UrlEncode(utf8Encode(text))
export const base64UrlDecodeText = (text: string): string => utf8Decode(base64UrlDecode(text))

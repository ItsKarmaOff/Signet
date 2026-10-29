import { fold } from '@signet/shared/urls'

/**
 * Range of `text` to show in bold for the typed `query`, or null. Same rules as the ranking: case and
 * accent insensitive, the occurrence at the start of the text wins, then a word start, then the first one.
 * Folding can change a character's length (accents), so indices are mapped back per character.
 */
export function highlightRange(text: string, query: string): [number, number] | null {
  const fq = fold(query.trim())
  if (!fq) return null
  let folded = ''
  // Original [start, end) of each folded character.
  const from: number[] = []
  const to: number[] = []
  for (let i = 0; i < text.length;) {
    const cp = text.codePointAt(i) as number
    const ch = String.fromCodePoint(cp)
    const f = fold(ch)
    for (let k = 0; k < f.length; k++) {
      from.push(i)
      to.push(i + ch.length)
    }
    folded += f
    i += ch.length
  }
  let at = -1
  for (let i = folded.indexOf(fq); i !== -1; i = folded.indexOf(fq, i + 1)) {
    if (at === -1) at = i
    if (i === 0 || !/[\p{L}\p{N}]/u.test(folded[i - 1])) {
      at = i
      break
    }
  }
  if (at === -1) return null
  let end = to[at + fq.length - 1]
  while (end < text.length && /\p{M}/u.test(text[end])) end++ // keep the accent attached to its letter
  return [from[at], end]
}

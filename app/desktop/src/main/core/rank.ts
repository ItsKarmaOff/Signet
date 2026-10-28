/**
 * Relevance of a page for what is typed in the command bar. Shared by the history search (which keeps
 * the best candidates) and the palette (which merges them with tabs, actions and the search row).
 * Scores go from 0 (no match) to about 100; `fq` and the fields are already folded (see `fold`).
 */

const SCHEME = /^[a-z][a-z0-9+.-]*:\/\//
const DAY = 86_400_000

/** Index of the first occurrence of `fq` in `s` that starts a word (not in the middle of one), or -1. */
function wordStart(s: string, fq: string): number {
  for (let i = s.indexOf(fq); i !== -1; i = s.indexOf(fq, i + 1)) if (i === 0 || !/[\p{L}\p{N}]/u.test(s[i - 1])) return i
  return -1
}

/**
 * How well the input matches a page, whatever its popularity. A match on the start of the host is much
 * stronger than one in the path or the title, and a word start beats the middle of a word.
 */
export function matchQuality(fq: string, title: string, url: string): number {
  if (!fq) return 0
  const bare = url.replace(SCHEME, '').replace(/^www\./, '')
  if (bare.startsWith(fq)) return 100
  const host = bare.split(/[/?#]/)[0].replace(/:\d+$/, '')
  if (host.split('.').some((label) => label.startsWith(fq))) return 80
  const inTitle = wordStart(title, fq)
  if (inTitle === 0) return 60
  if (inTitle > 0) return 45
  if (host.includes(fq)) return 40
  if (wordStart(bare, fq) >= 0) return 30
  return title.includes(fq) || bare.includes(fq) ? 15 : 0
}

/** Popularity between 0 and 1: frequency (saturates around 50 visits) and recency (halves every ~10 days). */
export function frecency(visits: number, lastVisit: number, now: number): number {
  const freq = Math.min(1, Math.log2(1 + Math.max(0, visits)) / Math.log2(51))
  const recency = Math.pow(0.5, Math.max(0, now - lastVisit) / DAY / 10)
  return 0.6 * freq + 0.4 * recency
}

/** Score of a history entry: the match decides, popularity moves it within 35% to 100% of that. */
export function historyScore(fq: string, e: { title: string; url: string; visits: number; lastVisit: number }, now: number): number {
  const q = matchQuality(fq, e.title, e.url)
  return q && q * (0.35 + 0.65 * frecency(e.visits, e.lastVisit, now))
}

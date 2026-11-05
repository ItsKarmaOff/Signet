// Keyword mode of the command bar: "yt " (or "yt" + Tab) turns the keyword into a chip, the rest is the query.
import type { KeywordInfo } from './searchengines'

const find = (keywords: readonly KeywordInfo[], token: string) => keywords.find((k) => k.keyword === token.toLowerCase())

/** Typed text starting with a keyword followed by whitespace (`yt cats`, `yt `): the keyword and the text after it. */
export function keywordPrefix(value: string, keywords: readonly KeywordInfo[]): { info: KeywordInfo; rest: string } | null {
  const m = /^\s*(\S+)\s([\s\S]*)$/.exec(value)
  const info = m && find(keywords, m[1])
  return info && m ? { info, rest: m[2].replace(/^\s+/, '') } : null
}

/** Typed text that is exactly a keyword (Tab enters the mode). */
export function keywordOnly(value: string, keywords: readonly KeywordInfo[]): KeywordInfo | null {
  const t = value.trim()
  return t && !/\s/.test(t) ? (find(keywords, t) ?? null) : null
}

/** Text sent to main for a query typed in keyword mode. */
export const keywordQuery = (info: KeywordInfo, query: string): string => `${info.keyword} ${query}`

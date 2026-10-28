/** The part of a result needed to complete the input. */
interface Completable {
  kind: string
  url?: string
}

/** Address without scheme, `www.` and trailing slash: what a user types to reach a site. */
export function bareUrl(url: string): string {
  return url
    .replace(/^[a-z][a-z0-9+.-]*:\/\//i, '')
    .replace(/^www\./i, '')
    .replace(/\/$/, '')
}

/**
 * Text to append (and show selected) after what is typed, or ''. Only a page row (tab, history,
 * archive) on a web address completes, and only when its bare address starts with the input, ignoring
 * case: the typed characters are kept as they are. Without a `/` typed, only the host is completed.
 */
export function inlineCompletion(typed: string, top: Completable | undefined): string {
  if (!typed || /\s/.test(typed) || !top?.url) return ''
  if (top.kind !== 'tab' && top.kind !== 'history' && top.kind !== 'archived') return ''
  if (!/^https?:\/\//i.test(top.url)) return ''
  let bare = bareUrl(top.url)
  if (!typed.includes('/')) bare = bare.split(/[/?#]/)[0]
  if (bare.length <= typed.length || !bare.toLowerCase().startsWith(typed.toLowerCase())) return ''
  return bare.slice(typed.length)
}

/**
 * Whether an edit may be completed: the text only grew at its end (a character typed), no composition,
 * not a paste. Deleting must never re-complete, or the completion could not be removed.
 */
export function canComplete(previous: string, next: string, inputType: string | undefined, composing: boolean): boolean {
  return !composing && inputType !== 'insertFromPaste' && next.length > previous.length && next.startsWith(previous)
}

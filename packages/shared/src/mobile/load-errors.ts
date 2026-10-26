// Classification of WebView load errors (iOS NSURLError and Android WebViewClient codes) to choose the error page message.

export type LoadErrorKind = 'offline' | 'notFound' | 'timeout' | 'secure' | 'refused' | 'other'

/** `code`: native code; `description`: native text (useful on Android, whose codes differ). */
export function classifyLoadError(code: number, description = ''): LoadErrorKind {
  const d = description.toLowerCase()
  if (code === -1009 || code === -1020 || /internet|offline|network is unreachable|net::err_internet_disconnected|err_network_changed/.test(d)) return 'offline'
  if (code === -1003 || code === -1002 || /not_resolved|could not be found|hostname|server with the specified/.test(d) || code === -2) return 'notFound'
  if (code === -1001 || code === -7 || /timed out|timeout/.test(d)) return 'timeout'
  if ((code <= -1200 && code >= -1206) || code === -11 || /ssl|certificate|cert_/.test(d)) return 'secure'
  if (code === -1004 || code === -6 || /refused|connection_refused|could not connect/.test(d)) return 'refused'
  return 'other'
}

/** Deliberate cancellation (the user started another page): no error to show. */
export const isCancelledLoad = (code: number): boolean => code === -999

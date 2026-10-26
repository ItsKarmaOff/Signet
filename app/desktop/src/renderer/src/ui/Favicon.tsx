import { useEffect, useState } from 'react'
import { Globe } from 'lucide-react'

/**
 * Favicons the image could not load (site headers, login required, no network): fetched by main, at
 * most once a minute. `null`: failure, globe in its place.
 */
const fallbacks = new Map<string, { data: string | null; at: number }>()
const RETRY_MS = 60_000
const MAX_FALLBACKS = 200

/** Remembers a result; the oldest drops out past the limit (data URLs are sometimes heavy). */
const remember = (src: string, data: string | null) => {
  fallbacks.delete(src)
  fallbacks.set(src, { data, at: Date.now() })
  if (fallbacks.size > MAX_FALLBACKS) fallbacks.delete(fallbacks.keys().next().value!)
}

const known = (src: string) => {
  const f = fallbacks.get(src)
  return f && (f.data || Date.now() - f.at < RETRY_MS) ? f : null
}

interface Props {
  src?: string
  size?: number
  loading?: boolean
}

export function Favicon({ src, size = 16, loading }: Props) {
  // The original address failed for this `src`: go through main.
  const [failed, setFailed] = useState<string | null>(null)
  const [, redraw] = useState(0)
  const cls = 'favicon' + (loading ? ' pulse' : '')
  const fallback = src ? known(src) : null
  const needsFallback = !!src && failed === src && !fallback

  useEffect(() => {
    if (!needsFallback || !src) return
    let live = true
    void window.signet
      .fallbackFavicon(src)
      .catch(() => null)
      .then((data) => {
        remember(src, data)
        if (live) redraw((n) => n + 1)
      })
    return () => {
      live = false
    }
  }, [needsFallback, src])

  const shown = fallback ? fallback.data : failed === src ? null : src
  if (!shown) return <Globe className={cls} size={size} strokeWidth={1.8} />
  return (
    <img
      className={cls}
      src={shown}
      width={size}
      height={size}
      alt=""
      decoding="async"
      draggable={false}
      onError={() => {
        if (shown === src) setFailed(src)
        else if (src) remember(src, null)
        redraw((n) => n + 1)
      }}
    />
  )
}

import { useEffect, useState } from 'react'
import type { CSSProperties } from 'react'
import type { Snapshot, Space, SpaceTheme } from '@shared/types'
import { IS_MAC } from '@signet/shared/i18n'
import { adoptPendingSpace, settleOptimistic } from './optimistic'

export const DRAG_MIME = 'application/x-signet-item'
/** Added when dragging a favorite: only display in the center is allowed. */
export const DRAG_FAVORITE_MIME = 'application/x-signet-favorite'
export const HUES = [0, 25, 45, 90, 150, 190, 215, 250, 285, 330]

/** Domain displayed for a URL (without `www.`). */
export function domainOf(url: string): string {
  if (/^signet:/i.test(url)) return url.replace(/\/+$/, '') // internal address: shown in full
  try {
    return new URL(url).hostname.replace(/^www\./, '') || url
  } catch {
    return url
  }
}

export const isSecure = (url: string): boolean => url.startsWith('https:')

/** Applies the space tint to the whole page (CSS variable `--h`). */
export function useHue(hue: number | undefined): void {
  useEffect(() => {
    if (hue !== undefined) document.documentElement.style.setProperty('--h', String(hue))
  }, [hue])
}

/** Strength of the gradient relative to the solid tint (multiplies the background tint amount). */
const GRADIENT_STRENGTH = 2.6

/**
 * Space background on the main window: accent tint (`--h`) and, with a gradient, its 2nd and 3rd hues, its strength
 * and its grain. A solid space sets the same variables with a single hue: the transition from one to the other animates.
 */
export function useSpaceTheme(space: Pick<Space, 'hue' | 'theme'> | undefined): void {
  const hues = space?.theme?.hues
  const key = space ? JSON.stringify([space.hue, hues, space.theme?.grain]) : ''
  useEffect(() => {
    if (!space) return
    const style = document.documentElement.style
    const [h1, h2, h3] = hues && hues.length >= 2 ? [hues[0], hues[1], hues[2] ?? hues[1]] : [space.hue, space.hue, space.hue]
    style.setProperty('--h', String(h1))
    style.setProperty('--h2', String(h2))
    style.setProperty('--h3', String(h3))
    style.setProperty('--gk', String(hues && hues.length >= 2 ? GRADIENT_STRENGTH : 1))
    style.setProperty('--grain', String(hues && hues.length >= 2 ? (space.theme?.grain ?? 0) : 0))
    // eslint-disable-next-line react-hooks/exhaustive-deps -- `key` summarizes the tint and the gradient
  }, [key])
}

/** Preview of a space background (form, Library): gradient of its hues, or its solid color. */
export function themeBackground(theme: SpaceTheme | undefined, hue: number): string {
  const hues = theme?.hues ?? []
  if (hues.length < 2) return `hsl(${hues[0] ?? hue} 70% 62%)`
  return `linear-gradient(135deg, ${hues.map((h) => `hsl(${h} 70% 62%)`).join(', ')})`
}

/**
 * Structural sharing: keeps the identity of `prev` for everything deeply equal in `next`
 * (plain data only: objects, arrays, primitives). Downstream memos then only see what changed.
 */
export function share<T>(prev: unknown, next: T): T {
  if (prev === next || !next || typeof next !== 'object' || !prev || typeof prev !== 'object') return next
  if (Array.isArray(next)) {
    if (!Array.isArray(prev)) return next
    // Lazy copy: we only allocate at the first difference.
    let out: unknown[] | null = null
    for (let i = 0; i < next.length; i++) {
      const r = share(prev[i], next[i])
      if (!out && r !== prev[i]) {
        out = []
        for (let j = 0; j < i; j++) out.push(prev[j])
      }
      if (out) out.push(r)
    }
    if (out) return out as T
    if (prev.length === next.length) return prev as T
    return (next.length < prev.length ? prev.slice(0, next.length) : [...prev, ...next.slice(prev.length)]) as T
  }
  if (Array.isArray(prev)) return next
  const p = prev as Record<string, unknown>
  const n = next as Record<string, unknown>
  const keys = Object.keys(n)
  let out: Record<string, unknown> | null = null
  for (let i = 0; i < keys.length; i++) {
    const k = keys[i]
    const r = share(p[k], n[k])
    if (!out && (r !== p[k] || !(k in p))) {
      out = {}
      for (let j = 0; j < i; j++) out[keys[j]] = p[keys[j]]
    }
    if (out) out[k] = r
  }
  if (out) return out as T
  if (Object.keys(p).length === keys.length) return prev as T
  const rest: Record<string, unknown> = {}
  for (const k of keys) rest[k] = p[k]
  return rest as T
}

/** Main window snapshot: initial read, then pushed updates. */
export function useSnapshot(): Snapshot | null {
  const [snap, setSnap] = useState<Snapshot | null>(null)
  useEffect(() => {
    let alive = true
    const off = window.signet.onState((s) => {
      setSnap((prev) => share(prev, s))
      settleOptimistic()
    })
    const offPatch = window.signet.onStatePatch((p) => {
      setSnap((prev) => {
        if (!prev) return prev
        let tabs: Snapshot['tabs'] | null = null
        for (const [id, part] of Object.entries(p.tabs)) {
          const cur = prev.tabs[id]
          if (!cur) continue
          tabs ??= { ...prev.tabs }
          tabs[id] = { ...cur, ...part }
        }
        return tabs ? { ...prev, tabs } : prev
      })
    })
    const offHint = window.signet.onPendingSpace?.(adoptPendingSpace)
    // A pushed state is always more recent than the initial read.
    window.signet
      .getState()
      .then((s) => alive && setSnap((prev) => prev ?? s))
      .catch(() => {})
    return () => {
      alive = false
      off()
      offPatch()
      offHint?.()
    }
  }, [])
  return snap
}

export { siteOf } from '@signet/shared/site'

/** Height of the pane bar (same value as `PANE_BAR` in main). */
export const PANE_BAR = 40
/** Height of the bookmarks bar, under the pane bar (same value as `BOOKMARK_BAR` in main). */
export const BOOKMARK_BAR = 32
/** Height of the header of the developer tools card (same value as `DEVTOOLS_BAR` in main). */
export const DEVTOOLS_BAR = 36

/** True on macOS: shortcuts are written with ⌘ there, with Ctrl elsewhere. */
export { IS_MAC }

/**
 * Shortcut as displayed, depending on the system: `shortcut('T', 'shift')` gives "⇧⌘T" on Mac, "Ctrl+Shift+T" elsewhere.
 * The app menus use `CmdOrCtrl`: ⌘ on Mac, Ctrl on Windows and Linux.
 */
export function shortcut(key: string, ...mods: ('shift' | 'alt')[]): string {
  if (IS_MAC) return `${mods.includes('alt') ? '⌥' : ''}${mods.includes('shift') ? '⇧' : ''}⌘${key}`
  return ['Ctrl', ...(mods.includes('alt') ? ['Alt'] : []), ...(mods.includes('shift') ? ['Shift'] : []), key].join('+')
}

/** Name of a language (`de` → "German") in the interface language; the code as is if unknown. */
export function languageName(code: string, locale: string): string {
  try {
    return new Intl.DisplayNames([locale], { type: 'language' }).of(code) ?? code
  } catch {
    return code
  }
}

/** Open in a new tab: ⌘click on Mac, Ctrl+click elsewhere, or middle click. */
export const wantsNewTab = (e: { metaKey: boolean; ctrlKey: boolean; button: number }) => (IS_MAC ? e.metaKey : e.ctrlKey) || e.button === 1

/** Relative luminance (0–1) of a `#rrggbb` color, or null if it is unreadable. */
export function luminance(hex: string): number | null {
  const m = /^#([0-9a-f]{6})/i.exec(hex)
  if (!m) return null
  const lin = (i: number) => {
    const c = parseInt(m[1].slice(i, i + 2), 16) / 255
    return c <= 0.03928 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4
  }
  return 0.2126 * lin(0) + 0.7152 * lin(2) + 0.0722 * lin(4)
}

/** Bar background = site color; light or dark content depending on contrast. No color: theme tokens. */
export function barStyle(color: string | undefined): CSSProperties | undefined {
  const l = color ? luminance(color) : null
  if (!color || l === null) return undefined
  // Light text only if it contrasts better than black (WCAG contrast ratios).
  const dark = 1.05 / (l + 0.05) > (l + 0.05) / 0.05
  return {
    '--bar-bg': color,
    '--bar-fg': dark ? 'rgb(255 255 255 / .92)' : 'rgb(0 0 0 / .85)',
    // Alpha driven by --bar-fg-2-alpha (0.5 by default; raised in increased contrast).
    '--bar-fg-2': dark ? 'rgb(255 255 255 / calc(var(--bar-fg-2-alpha) + .05))' : 'rgb(0 0 0 / var(--bar-fg-2-alpha))',
    '--bar-hover': dark ? 'rgb(255 255 255 / .1)' : 'rgb(0 0 0 / .1)'
  } as CSSProperties
}

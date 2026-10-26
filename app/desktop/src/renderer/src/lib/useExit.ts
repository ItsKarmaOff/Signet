import { useEffect, useState } from 'react'

/** Duration of a surface's exit (= --t-fast in styles.css; OVERLAY_EXIT_MS on the main side). */
export const EXIT_MS = 120

/**
 * Keeps a panel mounted for the duration of its animated exit: `value` stays the last non-null value
 * for `ms` after it disappears, and `closing` is true during that time. A new value cancels the exit.
 */
export function useExit<T>(value: T | null | undefined | false, ms = EXIT_MS): { value: T | null; closing: boolean } {
  const [kept, setKept] = useState<T | null>(value || null)
  useEffect(() => {
    if (value) {
      setKept(value)
      return
    }
    const id = setTimeout(() => setKept(null), ms)
    return () => clearTimeout(id)
  }, [value, ms])
  const shown = value || kept
  return { value: shown || null, closing: !value && !!kept }
}

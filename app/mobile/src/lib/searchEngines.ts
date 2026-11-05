// Search engines as the app sees them: built-ins, presets (YouTube, Wikipedia, GitHub) and the user's own, with their keywords.
import { useMemo } from 'react'
import { engineById, resolveEngines, type EngineSpec } from '@signet/shared/searchengines'
import { useLocale } from '@/lib/i18n'
import { useStore } from '@/state'

/** Engine icon: DuckDuckGo's icon service (one request per engine, cached); the colored letter takes over offline. */
export const engineIcon = (template: string): string | undefined => {
  try {
    return `https://icons.duckduckgo.com/ip3/${new URL(template.split('%s').join('x')).hostname}.ico`
  } catch {
    return undefined
  }
}

/** Every engine (the Wikipedia host follows the app language) and the default one (Google if it is gone or turned off). */
export function useSearchEngines(): { engines: EngineSpec[]; defaultEngine: EngineSpec } {
  const locale = useLocale()
  const custom = useStore((s) => s.settings.searchEngines)
  const keywords = useStore((s) => s.settings.searchKeywords)
  const id = useStore((s) => s.settings.searchEngine)
  return useMemo(() => {
    const engines = resolveEngines({ searchEngines: custom, searchKeywords: keywords }, locale)
    return { engines, defaultEngine: engineById(engines, id) }
  }, [custom, keywords, id, locale])
}

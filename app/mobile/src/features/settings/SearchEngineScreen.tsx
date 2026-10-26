import { SEARCH_ENGINES } from '@signet/shared/mobile/search'
import { useT } from '@/lib/i18n'
import { useStore } from '@/state'
import { Favicon, ListRow, ListSection, Screen, SwitchRow, space } from '@/ui'

/** Engine icon: DuckDuckGo's icon service (one request per engine, cached); the colored letter takes over offline. */
const engineIcon = (template: string): string | undefined => {
  try {
    return `https://icons.duckduckgo.com/ip3/${new URL(template).hostname}.ico`
  } catch {
    return undefined
  }
}

export function SearchEngineScreen() {
  const t = useT()
  const current = useStore((s) => s.settings.searchEngine)
  const setSettings = useStore((s) => s.setSettings)
  const suggestions = useStore((s) => s.settings.searchSuggestions)
  return (
    <Screen scroll contentStyle={{ paddingTop: space.lg }}>
      <ListSection footer={t('settings.searchEngineFooter')}>
        {SEARCH_ENGINES.map((e) => (
          <ListRow
            key={e.id}
            title={e.name}
            left={<Favicon uri={engineIcon(e.template)} url={e.template} size={29} />}
            selected={e.id === current}
            onPress={() => setSettings({ searchEngine: e.id })}
          />
        ))}
      </ListSection>
      <ListSection footer={t('settings.searchSuggestionsFooter')}>
        <SwitchRow title={t('settings.searchSuggestions')} value={suggestions} onValueChange={(searchSuggestions) => setSettings({ searchSuggestions })} />
      </ListSection>
    </Screen>
  )
}

import { useRouter } from 'expo-router'
import { engineIcon, useSearchEngines } from '@/lib/searchEngines'
import { useT } from '@/lib/i18n'
import { useStore } from '@/state'
import { Favicon, ListRow, ListSection, Screen, SwitchRow, space } from '@/ui'

export function SearchEngineScreen() {
  const t = useT()
  const router = useRouter()
  const { engines, defaultEngine } = useSearchEngines()
  const setSettings = useStore((s) => s.setSettings)
  const suggestions = useStore((s) => s.settings.searchSuggestions)
  return (
    <Screen scroll contentStyle={{ paddingTop: space.lg }}>
      <ListSection footer={t('settings.searchEngineFooter')}>
        {engines
          .filter((e) => e.enabled)
          .map((e) => (
            <ListRow
              key={e.id}
              title={e.name}
              left={<Favicon uri={engineIcon(e.template)} url={e.template} size={29} />}
              selected={e.id === defaultEngine.id}
              onPress={() => setSettings({ searchEngine: e.id })}
            />
          ))}
      </ListSection>
      <ListSection>
        <ListRow title={t('settings.enginesAndKeywords')} onPress={() => router.push('/settings/search-keywords')} chevron />
      </ListSection>
      <ListSection footer={t('settings.searchSuggestionsFooter')}>
        <SwitchRow title={t('settings.searchSuggestions')} value={suggestions} onValueChange={(searchSuggestions) => setSettings({ searchSuggestions })} />
      </ListSection>
    </Screen>
  )
}

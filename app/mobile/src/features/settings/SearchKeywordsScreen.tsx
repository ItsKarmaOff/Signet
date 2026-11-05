// Settings > Search engine > Engines and keywords: every engine with its keyword; presets can be turned off, a tap edits an engine.
import { View } from 'react-native'
import { useRouter } from 'expo-router'
import { CUSTOM_ENGINES_MAX, withOverride } from '@signet/shared/searchengines'
import { useT } from '@/lib/i18n'
import { engineIcon, useSearchEngines } from '@/lib/searchEngines'
import { useStore } from '@/state'
import { Favicon, ListRow, ListSection, Screen, Text, Toggle, icons, space } from '@/ui'

export function SearchKeywordsScreen() {
  const t = useT()
  const router = useRouter()
  const { engines, defaultEngine } = useSearchEngines()
  const keywords = useStore((s) => s.settings.searchKeywords)
  const setSettings = useStore((s) => s.setSettings)
  const customCount = engines.filter((e) => e.source === 'custom').length
  const edit = (id: string) => router.push({ pathname: '/settings/search-engine-edit', params: { id } })
  return (
    <Screen scroll contentStyle={{ paddingTop: space.lg, paddingBottom: space.huge }}>
      <ListSection title={t('engines.engines')} footer={t('engines.intro')}>
        {engines.map((e) => (
          <ListRow
            key={e.id}
            title={e.name}
            subtitle={e.id === defaultEngine.id ? `${t(`engines.${e.source}`)} · ${t('engines.default')}` : t(`engines.${e.source}`)}
            left={<Favicon uri={engineIcon(e.template)} url={e.template} size={29} />}
            right={
              <View style={{ flexDirection: 'row', alignItems: 'center', gap: space.md }}>
                <Text variant="body" color={e.enabled ? 'secondary' : 'tertiary'} numberOfLines={1}>
                  {e.keyword}
                </Text>
                {e.source === 'preset' ? (
                  <Toggle
                    value={e.enabled}
                    accessibilityLabel={e.name}
                    onValueChange={(on) => setSettings({ searchKeywords: withOverride(keywords, e, { disabled: !on }) })}
                  />
                ) : null}
              </View>
            }
            chevron
            onPress={() => edit(e.id)}
          />
        ))}
      </ListSection>
      {customCount < CUSTOM_ENGINES_MAX ? (
        <ListSection>
          <ListRow title={t('engines.add')} icon={icons.Plus} onPress={() => edit('new')} />
        </ListSection>
      ) : null}
    </Screen>
  )
}

// Add or edit a search engine. `id=new`: new custom engine. Built-ins and presets: only the keyword can change.
import { useState } from 'react'
import { View } from 'react-native'
import { Stack, useLocalSearchParams, useRouter } from 'expo-router'
import { newId } from '@signet/shared/ids'
import { normalizeKeyword, validateEngineDraft, withOverride, type CustomEngine, type EngineDraftError } from '@signet/shared/searchengines'
import { haptic } from '@/lib/haptics'
import { useT } from '@/lib/i18n'
import { useSearchEngines } from '@/lib/searchEngines'
import { useStore } from '@/state'
import { ActionSheet, Button, EmptyState, ListRow, ListSection, Screen, TextField, Text, gutter, icons, space, toast } from '@/ui'

const ERROR_KEYS = {
  name: 'engines.errorName',
  keyword: 'engines.errorKeyword',
  keywordTaken: 'engines.errorKeywordTaken',
  template: 'engines.errorTemplate',
  suggest: 'engines.errorSuggest'
} as const satisfies Record<EngineDraftError, string>

/** The form field an error belongs to. */
const FIELD_OF: Record<EngineDraftError, 'name' | 'keyword' | 'template' | 'suggest'> = {
  name: 'name',
  keyword: 'keyword',
  keywordTaken: 'keyword',
  template: 'template',
  suggest: 'suggest'
}

export function SearchEngineEditScreen() {
  const t = useT()
  const router = useRouter()
  const { id } = useLocalSearchParams<{ id?: string }>()
  const isNew = id === 'new'
  const { engines } = useSearchEngines()
  const custom = useStore((s) => s.settings.searchEngines)
  const keywords = useStore((s) => s.settings.searchKeywords)
  const setSettings = useStore((s) => s.setSettings)
  const engine = isNew ? undefined : engines.find((e) => e.id === id)
  const isCustom = isNew || engine?.source === 'custom'
  const [name, setName] = useState(engine?.name ?? '')
  const [keyword, setKeyword] = useState(engine?.keyword ?? '')
  const [template, setTemplate] = useState(engine?.template ?? '')
  const [suggest, setSuggest] = useState(engine?.suggest ?? '')
  const [error, setError] = useState<EngineDraftError | null>(null)
  const [confirmDelete, setConfirmDelete] = useState(false)

  const done = (message: string) => {
    haptic('success')
    router.back()
    toast.show({ message })
  }

  const save = () => {
    if (isCustom) {
      const draft = { name, keyword, template, suggest }
      const err = validateEngineDraft(draft, engines, engine?.id)
      if (err) return setError(err)
      const extra = suggest.trim()
      const entry: CustomEngine = {
        id:
          engine?.id ??
          `custom-${newId()
            .replace(/[^a-z0-9]/gi, '')
            .toLowerCase()
            .slice(0, 12)}`,
        name: name.trim(),
        keyword: normalizeKeyword(keyword)!,
        template: template.trim(),
        ...(extra && { suggest: extra })
      }
      setSettings({ searchEngines: engine ? custom.map((c) => (c.id === engine.id ? entry : c)) : [...custom, entry] })
    } else if (engine) {
      // Empty: back to the default keyword.
      const k = keyword.trim() === '' && engine.defaultKeyword ? engine.defaultKeyword : normalizeKeyword(keyword)
      if (!k) return setError('keyword')
      if (engines.some((o) => o.id !== engine.id && o.keyword === k)) return setError('keywordTaken')
      setSettings({ searchKeywords: withOverride(keywords, engine, { keyword: k }) })
    }
    done(t('engines.saved'))
  }

  const remove = () => {
    setConfirmDelete(false)
    if (!engine) return
    // The default falls back to Google when it was this engine (settings are sanitized).
    setSettings({ searchEngines: custom.filter((c) => c.id !== engine.id) })
    done(t('engines.deleted'))
  }

  const header = (
    <Stack.Screen
      options={{
        title: isNew ? t('engines.addTitle') : t('engines.editTitle'),
        headerRight: () => (isNew || engine ? <Button title={t('engines.save')} variant="plain" onPress={save} /> : null)
      }}
    />
  )
  if (!isNew && !engine) {
    return (
      <Screen>
        {header}
        <EmptyState icon={icons.Search} title={t('engines.notFound')} />
      </Screen>
    )
  }

  const err = (field: (typeof FIELD_OF)[EngineDraftError]) => (error && FIELD_OF[error] === field ? t(ERROR_KEYS[error]) : undefined)
  const edit = (set: (v: string) => void) => (v: string) => {
    set(v)
    setError(null)
  }

  return (
    <Screen scroll contentStyle={{ paddingTop: space.lg, paddingBottom: space.huge }}>
      {header}
      <View style={{ paddingHorizontal: gutter, gap: space.lg }}>
        {isCustom ? (
          <TextField
            label={t('engines.name')}
            value={name}
            onChangeText={edit(setName)}
            placeholder={t('engines.namePlaceholder')}
            error={err('name')}
            autoFocus={isNew}
            maxLength={80}
          />
        ) : (
          <ListSection>
            <ListRow title={engine?.name ?? ''} subtitle={t(`engines.${engine?.source ?? 'builtin'}`)} />
          </ListSection>
        )}
        <TextField
          label={t('engines.keyword')}
          value={keyword}
          onChangeText={edit(setKeyword)}
          placeholder={engine?.defaultKeyword ?? t('engines.keyword')}
          error={err('keyword')}
          autoCapitalize="none"
          autoCorrect={false}
          maxLength={64}
        />
        {isCustom ? (
          <>
            <TextField
              label={t('engines.url')}
              value={template}
              onChangeText={edit(setTemplate)}
              placeholder={t('engines.urlPlaceholder')}
              error={err('template')}
              autoCapitalize="none"
              autoCorrect={false}
              keyboardType="url"
            />
            <TextField
              label={t('engines.suggestUrl')}
              value={suggest}
              onChangeText={edit(setSuggest)}
              placeholder={t('engines.suggestPlaceholder')}
              error={err('suggest')}
              autoCapitalize="none"
              autoCorrect={false}
              keyboardType="url"
            />
            <Text variant="footnote" color="secondary">
              {t('engines.formHint')}
            </Text>
          </>
        ) : (
          <Text variant="footnote" color="secondary">
            {t('engines.keywordFooter')}
          </Text>
        )}
      </View>
      {engine && isCustom ? (
        <View style={{ marginTop: space.xxl }}>
          <ListSection>
            <ListRow title={t('engines.delete')} icon={icons.Trash} destructive onPress={() => setConfirmDelete(true)} />
          </ListSection>
        </View>
      ) : null}
      <ActionSheet
        visible={confirmDelete}
        onClose={() => setConfirmDelete(false)}
        title={t('engines.deleteTitle')}
        message={t('engines.deleteMessage')}
        cancelLabel={t('common.cancel')}
        actions={[{ label: t('engines.delete'), destructive: true, onPress: remove }]}
      />
    </Screen>
  )
}

import { useEffect, useRef, useState } from 'react'
import { View } from 'react-native'
import { parseHomepageInput, type HomepageMode } from '@signet/shared/mobile/settings'
import { useT } from '@/lib/i18n'
import { useStore } from '@/state'
import { ListRow, ListSection, Screen, TextField, gutter, space } from '@/ui'

export function HomepageScreen() {
  const t = useT()
  const homepage = useStore((s) => s.settings.homepage)
  const homepageUrl = useStore((s) => s.settings.homepageUrl)
  const setSettings = useStore((s) => s.setSettings)
  // The "chosen address" setting without a valid address falls back to the start page: the current choice is therefore kept here.
  const [mode, setMode] = useState<HomepageMode>(homepage)
  const [text, setText] = useState(homepageUrl)
  const [touched, setTouched] = useState(false)
  const latest = useRef({ mode, text })
  latest.current = { mode, text }

  const commit = () => {
    const { mode: m, text: value } = latest.current
    if (m !== 'custom') return
    const url = parseHomepageInput(value)
    if (url) setSettings({ homepage: 'custom', homepageUrl: url })
  }
  // Leaving the screen saves a valid address that was typed but not yet confirmed on the keyboard.
  useEffect(() => commit, []) // eslint-disable-line react-hooks/exhaustive-deps

  const choose = (next: HomepageMode) => {
    setMode(next)
    if (next === 'custom') {
      const url = parseHomepageInput(text)
      if (url) setSettings({ homepage: 'custom', homepageUrl: url })
    } else setSettings({ homepage: next })
  }

  const invalid = mode === 'custom' && touched && text.trim().length > 0 && !parseHomepageInput(text)
  return (
    <Screen scroll contentStyle={{ paddingTop: space.lg }}>
      <ListSection footer={t('settings.homepageFooter')}>
        <ListRow title={t('settings.homepageStart')} selected={mode === 'start'} onPress={() => choose('start')} />
        <ListRow title={t('settings.homepageBlank')} selected={mode === 'blank'} onPress={() => choose('blank')} />
        <ListRow title={t('settings.homepageCustom')} selected={mode === 'custom'} onPress={() => choose('custom')} />
      </ListSection>
      {mode === 'custom' ? (
        <View style={{ paddingHorizontal: gutter }}>
          <TextField
            label={t('settings.homepageUrl')}
            value={text}
            onChangeText={setText}
            onBlur={() => {
              setTouched(true)
              commit()
            }}
            onSubmitEditing={() => {
              setTouched(true)
              commit()
            }}
            placeholder={t('settings.homepageUrlPlaceholder')}
            error={invalid ? t('settings.homepageUrlInvalid') : undefined}
            autoCapitalize="none"
            autoCorrect={false}
            keyboardType="url"
            returnKeyType="done"
            autoFocus={!homepageUrl}
          />
        </View>
      ) : null}
    </Screen>
  )
}

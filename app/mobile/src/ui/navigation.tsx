// expo-router stack options in the theme colors: pass to <Stack screenOptions={useStackOptions()} />.
import type { ComponentProps } from 'react'
import { useRouter, type Stack } from 'expo-router'
import { useT } from '@/lib/i18n'
import { Button } from './Button'
import { fontStyle } from './tokens'
import { useColors } from './theme'

type StackOptions = Extract<NonNullable<ComponentProps<typeof Stack>['screenOptions']>, object>

export function useStackOptions(): StackOptions {
  const c = useColors()
  return {
    headerStyle: { backgroundColor: c.background },
    headerTintColor: c.accent,
    headerTitleStyle: { color: c.text, fontWeight: '600', fontSize: fontStyle('headline').fontSize },
    headerShadowVisible: false,
    headerBackButtonDisplayMode: 'minimal',
    contentStyle: { backgroundColor: c.background }
  }
}

/** Header "OK" button to close a stack presented as a modal (library, settings). */
export function HeaderDone() {
  const t = useT()
  const router = useRouter()
  return <Button title={t('common.done')} variant="plain" onPress={() => router.dismiss()} />
}

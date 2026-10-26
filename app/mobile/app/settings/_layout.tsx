import { Stack } from 'expo-router'
import { View } from 'react-native'
import { useT } from '@/lib/i18n'
import { HeaderDone, ToastHost, space, useStackOptions } from '@/ui'

export default function SettingsLayout() {
  const t = useT()
  const options = useStackOptions()
  return (
    <View style={{ flex: 1 }}>
      <Stack screenOptions={options}>
        <Stack.Screen name="index" options={{ title: t('settings.title'), headerRight: () => <HeaderDone /> }} />
        <Stack.Screen name="search-engine" options={{ title: t('settings.searchEngine') }} />
        <Stack.Screen name="homepage" options={{ title: t('settings.homepage') }} />
        <Stack.Screen name="app-icon" options={{ title: t('settings.appIcon') }} />
        <Stack.Screen name="data" options={{ title: t('settings.data') }} />
        <Stack.Screen name="transfer" options={{ title: t('settings.importExport') }} />
        <Stack.Screen name="passwords" options={{ title: t('passwords.title') }} />
        <Stack.Screen name="passwords-detail" options={{ title: t('passwords.title') }} />
        <Stack.Screen name="passwords-never" options={{ title: t('passwords.never') }} />
        <Stack.Screen name="computers" options={{ title: t('link.title') }} />
        <Stack.Screen name="pair" options={{ title: t('link.pair') }} />
        <Stack.Screen name="pair-confirm" options={{ title: t('link.pair'), gestureEnabled: false }} />
        <Stack.Screen name="about" options={{ title: t('settings.about') }} />
      </Stack>
      {/* Settings is a native modal: the root toast would show behind it, so we mount our own. */}
      <ToastHost bottomOffset={space.lg} />
    </View>
  )
}

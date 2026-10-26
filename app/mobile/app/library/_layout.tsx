import { Stack } from 'expo-router'
import { View } from 'react-native'
import { useT } from '@/lib/i18n'
import { HeaderDone, ToastHost, space, useStackOptions } from '@/ui'

export default function LibraryLayout() {
  const t = useT()
  const options = useStackOptions()
  return (
    <View style={{ flex: 1 }}>
      <Stack screenOptions={options}>
        <Stack.Screen name="index" options={{ title: t('library.title'), headerRight: () => <HeaderDone /> }} />
        <Stack.Screen name="history" options={{ title: t('library.history') }} />
        <Stack.Screen name="downloads" options={{ title: t('library.downloads') }} />
        <Stack.Screen name="bookmarks/index" options={{ title: t('library.bookmarks') }} />
        <Stack.Screen name="bookmarks/[folderId]" options={{ title: t('library.bookmarks') }} />
      </Stack>
      {/* The library is a native modal: the root toast would show behind it, so we mount our own. */}
      <ToastHost bottomOffset={space.lg} />
    </View>
  )
}

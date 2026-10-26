// Root: providers (gestures, safe areas, theme), storage read, main stack.
// Everything is here except the screens: each route in `app/` is a thin file that re-exports a screen from `src/features/`.
import { useEffect } from 'react'
import { View } from 'react-native'
import { GestureHandlerRootView } from 'react-native-gesture-handler'
import { SafeAreaProvider } from 'react-native-safe-area-context'
import { Stack } from 'expo-router'
import { StatusBar } from 'expo-status-bar'
import { useQaCommands } from '@/dev/useQaCommands'
import { LinkHost, useLinkManager } from '@/features/link'
import { useSyncAppIcon } from '@/features/settings'
import { PrivateLockOverlay } from '@/features/tabs'
import { useT } from '@/lib/i18n'
import { bootstrapStore, useStore } from '@/state'
import { ThemeProvider, ToastHost, useStackOptions, useTheme } from '@/ui'

export default function RootLayout() {
  const hydrated = useStore((s) => s.hydrated)
  useEffect(() => {
    void bootstrapStore()
  }, [])
  return (
    <GestureHandlerRootView style={{ flex: 1 }}>
      <SafeAreaProvider>
        <ThemeProvider>{hydrated ? <Shell /> : <Splash />}</ThemeProvider>
      </SafeAreaProvider>
    </GestureHandlerRootView>
  )
}

/** While storage is being read (a few ms): background only, no flicker. */
function Splash() {
  const { colors } = useTheme()
  return <View style={{ flex: 1, backgroundColor: colors.background }} />
}

function Shell() {
  const t = useT()
  const { scheme } = useTheme()
  const onboardingDone = useStore((s) => s.ui.onboardingDone)
  const options = useStackOptions()
  useSyncAppIcon()
  useLinkManager()
  useQaCommands()
  return (
    <>
      <StatusBar style={scheme === 'dark' ? 'light' : 'dark'} />
      <Stack screenOptions={{ ...options, headerShown: false }}>
        {/* First launch: only the welcome screen exists; afterwards it disappears from the stack. */}
        <Stack.Protected guard={!onboardingDone}>
          <Stack.Screen name="onboarding" options={{ gestureEnabled: false }} />
        </Stack.Protected>
        <Stack.Protected guard={onboardingDone}>
          <Stack.Screen name="index" />
          {/* Also guarded: otherwise, before the welcome screen, "/" falls through to /open, which redirects to "/" in a loop. */}
          <Stack.Screen name="open" />
          <Stack.Screen name="tabs" options={{ presentation: 'fullScreenModal', animation: 'slide_from_bottom' }} />
          <Stack.Screen name="command" options={{ presentation: 'transparentModal', animation: 'fade' }} />
          <Stack.Screen name="library" options={{ presentation: 'modal' }} />
          <Stack.Screen name="settings" options={{ presentation: 'modal' }} />
          <Stack.Screen
            name="link"
            options={{ presentation: 'formSheet', sheetAllowedDetents: [0.55], sheetGrabberVisible: true, contentStyle: { backgroundColor: 'transparent' } }}
          />
          <Stack.Screen
            name="spaces/edit/[id]"
            options={{
              presentation: 'formSheet',
              title: t('spaces.edit'),
              sheetAllowedDetents: [0.6, 1],
              sheetGrabberVisible: true,
              contentStyle: { backgroundColor: 'transparent' }
            }}
          />
        </Stack.Protected>
      </Stack>
      <ToastHost />
      <PrivateLockOverlay />
      <LinkHost />
    </>
  )
}

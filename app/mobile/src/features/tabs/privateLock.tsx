// Private tabs lock (`privateLock` setting): when the app goes to the background, private tabs lock.
// On return, a blurred "Unlock" screen asks for Face ID / Touch ID or the device passcode (expo-local-authentication).
// While passing through the iOS app switcher ("inactive" state), a veil also hides private pages.
// Without a device lock (neither biometrics nor passcode), there is nothing to ask: the lock opens.
import { useEffect } from 'react'
import { AppState, Platform, StyleSheet, View } from 'react-native'
import { BlurView } from 'expo-blur'
import * as LocalAuthentication from 'expo-local-authentication'
import { create } from 'zustand'
import { haptic } from '@/lib/haptics'
import { t, useT } from '@/lib/i18n'
import { useStore } from '@/state'
import { Button, Text, icons, radius, space, useTheme } from '@/ui'

interface LockState {
  /** Private tabs are locked (the app went to the background). */
  locked: boolean
  /** Temporary veil (app not active: app switcher, Face ID prompt). */
  covered: boolean
  authenticating: boolean
}

const useLock = create<LockState>(() => ({ locked: false, covered: false, authenticating: false }))

const lockEnabled = () => useStore.getState().settings.privateLock
const hasPrivateTabs = () => useStore.getState().workspace.priv.tabIds.length > 0

let installed = 0
let unsubscribers: (() => void)[] = []

function install(): () => void {
  if (installed++ === 0) {
    const sub = AppState.addEventListener('change', (next) => {
      if (!lockEnabled() || !hasPrivateTabs()) {
        useLock.setState({ covered: false })
        return
      }
      if (next === 'background') useLock.setState({ locked: true, covered: true })
      else if (next === 'inactive') useLock.setState({ covered: true })
      else useLock.setState({ covered: false })
    })
    // No more private tabs: nothing left to protect.
    const unsub = useStore.subscribe((state) => {
      if (state.workspace.priv.tabIds.length === 0 && useLock.getState().locked) useLock.setState({ locked: false })
    })
    unsubscribers = [() => sub.remove(), unsub]
  }
  return () => {
    if (--installed === 0) {
      unsubscribers.forEach((u) => u())
      unsubscribers = []
    }
  }
}

/** Asks the device to confirm identity. True if done (or if there is no lock to ask for). */
export async function unlockPrivate(): Promise<boolean> {
  if (!useLock.getState().locked) return true
  if (useLock.getState().authenticating) return false
  useLock.setState({ authenticating: true })
  try {
    const level = await LocalAuthentication.getEnrolledLevelAsync().catch(() => LocalAuthentication.SecurityLevel.NONE)
    let ok = level === LocalAuthentication.SecurityLevel.NONE
    if (!ok) {
      const result = await LocalAuthentication.authenticateAsync({ promptMessage: t('private.unlockReason'), cancelLabel: t('common.cancel') })
      ok = result.success
    }
    if (ok) {
      useLock.setState({ locked: false, covered: false })
      haptic('success')
    }
    return ok
  } catch {
    return false
  } finally {
    useLock.setState({ authenticating: false })
  }
}

/**
 * Switching to private browsing: if private tabs are locked, asks for identity first.
 * Call before `setPrivateMode(true)`; returns true if it is OK to proceed.
 */
export async function requestPrivateAccess(): Promise<boolean> {
  if (!lockEnabled() || !hasPrivateTabs()) return true
  return unlockPrivate()
}

/** Private tabs lock screen: mount in `app/_layout.tsx` and in any native modal that can show private pages. */
export function PrivateLockOverlay() {
  const tr = useT()
  const { colors } = useTheme()
  const privateMode = useStore((s) => s.workspace.privateMode)
  const enabled = useStore((s) => s.settings.privateLock)
  const setPrivateMode = useStore((s) => s.setPrivateMode)
  const { locked, covered, authenticating } = useLock()
  const visible = privateMode && enabled && (locked || covered)

  useEffect(install, [])
  // Automatic prompt on return to the app (once), not during the app switcher veil.
  useEffect(() => {
    if (privateMode && enabled && locked && !covered && !authenticating) void unlockPrivate()
    // eslint-disable-next-line react-hooks/exhaustive-deps -- only when the lock or veil state changes
  }, [privateMode, enabled, locked, covered])

  if (!visible) return null
  const blur = Platform.OS === 'ios'
  return (
    <View style={[StyleSheet.absoluteFill, styles.host]} accessibilityViewIsModal>
      {blur ? <BlurView intensity={90} tint="dark" style={StyleSheet.absoluteFill} /> : null}
      <View style={[StyleSheet.absoluteFill, { backgroundColor: blur ? 'rgba(20,18,26,0.55)' : colors.background }]} />
      {locked ? (
        <View style={styles.content}>
          <View style={[styles.lock, { backgroundColor: colors.surface2 }]}>
            <icons.LockKeyhole size={30} color={colors.accent} strokeWidth={1.8} />
          </View>
          <Text variant="title3" align="center">
            {tr('private.lockedTitle')}
          </Text>
          <Text variant="subhead" color="secondary" align="center">
            {tr('private.lockedHint')}
          </Text>
          <View style={styles.actions}>
            <Button
              title={tr('private.unlock')}
              icon={Platform.OS === 'android' ? icons.FingerprintPattern : icons.ScanFace}
              size="lg"
              fullWidth
              loading={authenticating}
              onPress={() => void unlockPrivate()}
            />
            <Button title={tr('private.leave')} variant="plain" onPress={() => setPrivateMode(false)} />
          </View>
        </View>
      ) : null}
    </View>
  )
}

const styles = StyleSheet.create({
  host: { zIndex: 1000, elevation: 1000, alignItems: 'center', justifyContent: 'center' },
  content: { width: '100%', maxWidth: 360, paddingHorizontal: space.xxl, alignItems: 'center', gap: space.sm },
  lock: { width: 72, height: 72, borderRadius: radius.card, alignItems: 'center', justifyContent: 'center', marginBottom: space.md },
  actions: { alignSelf: 'stretch', gap: space.xs, marginTop: space.xl, alignItems: 'center' }
})

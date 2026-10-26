// Settings › Computers › Pair: reads the QR code shown by the computer, then moves on to confirmation.
// iOS: in-screen camera (AVFoundation reads the code easily).
// Android: Google's code scanner (Play Services), the same engine as the system scanner. CameraView's
// analysis works on a low-resolution image there and did not detect the code shown on the screen of
// the computer. Without Play Services, or if the scanner fails, we fall back to the in-screen camera.
import { useCallback, useEffect, useRef, useState } from 'react'
import { Linking, Platform, StyleSheet, View } from 'react-native'
import { CameraView, useCameraPermissions } from 'expo-camera'
import { useRouter } from 'expo-router'
import { PAIRING_URL_PREFIX } from '@signet/shared/link/protocol'
import { useT } from '@/lib/i18n'
import { EmptyState, Screen, Text, gutter, icons, radius, space, useColors } from '@/ui'

/** Reading a code: moves on to confirmation if it is a pairing code, otherwise flags it for 2.5 s. */
function usePairingCode(): { onData: (data: string) => void; wrong: boolean } {
  const router = useRouter()
  const handled = useRef(false)
  const [wrong, setWrong] = useState(false)

  useEffect(() => {
    if (!wrong) return
    const timer = setTimeout(() => setWrong(false), 2500)
    return () => clearTimeout(timer)
  }, [wrong])

  const onData = useCallback(
    (data: string) => {
      if (handled.current) return
      if (!data.startsWith(PAIRING_URL_PREFIX)) {
        setWrong(true)
        return
      }
      handled.current = true
      router.replace({ pathname: '/settings/pair-confirm', params: { d: data.slice(PAIRING_URL_PREFIX.length) } })
    },
    [router]
  )
  return { onData, wrong }
}

export function PairScannerScreen() {
  const [fallback, setFallback] = useState(false)
  const useGoogle = Platform.OS === 'android' && CameraView.isModernBarcodeScannerAvailable && !fallback
  return useGoogle ? <GoogleScanner onUnavailable={() => setFallback(true)} /> : <CameraScanner />
}

/** Android: Google's scanner opens over the app; the screen offers to reopen it if it was closed. */
function GoogleScanner({ onUnavailable }: { onUnavailable: () => void }) {
  const t = useT()
  const { onData, wrong } = usePairingCode()
  const latest = useRef({ onData, onUnavailable })
  latest.current = { onData, onUnavailable }

  const launch = useCallback(() => {
    // First use: Play Services may still be downloading the scanner module, hence the possible failure.
    CameraView.launchScanner({ barcodeTypes: ['qr'] }).catch(() => latest.current.onUnavailable())
  }, [])

  useEffect(() => {
    const sub = CameraView.onModernBarcodeScanned((e) => latest.current.onData(e.data))
    launch()
    return () => sub.remove()
  }, [launch])

  return (
    <Screen>
      <View style={styles.center}>
        <EmptyState
          icon={icons.QrCode}
          title={t('link.scanTitle')}
          message={wrong ? t('link.scanWrong') : t('link.scanHint')}
          action={{ label: t('link.scanAgain'), onPress: launch }}
        />
      </View>
    </Screen>
  )
}

/** In-screen camera (iOS, and Android without Play Services). */
function CameraScanner() {
  const t = useT()
  const c = useColors()
  const [permission, requestPermission] = useCameraPermissions()
  const { onData, wrong } = usePairingCode()

  if (!permission) return <Screen />
  if (!permission.granted) {
    return (
      <Screen>
        <View style={styles.center}>
          <EmptyState
            icon={icons.QrCode}
            title={t('link.cameraTitle')}
            message={permission.canAskAgain ? t('link.cameraAsk') : t('link.cameraDenied')}
            action={
              permission.canAskAgain
                ? { label: t('link.cameraAllow'), onPress: () => void requestPermission() }
                : { label: t('link.cameraSettings'), onPress: () => void Linking.openSettings() }
            }
          />
        </View>
      </Screen>
    )
  }

  return (
    <Screen padded>
      <Text variant="subhead" color="secondary" align="center" style={styles.hint}>
        {t('link.scanHint')}
      </Text>
      <View style={[styles.camera, { backgroundColor: c.surface }]}>
        <CameraView
          style={StyleSheet.absoluteFill}
          facing="back"
          barcodeScannerSettings={{ barcodeTypes: ['qr'] }}
          onBarcodeScanned={({ data }) => onData(data)}
        />
        <View pointerEvents="none" style={styles.frameWrap}>
          <View style={[styles.frame, { borderColor: wrong ? c.danger : '#ffffff' }]} />
        </View>
      </View>
      <Text variant="footnote" color={wrong ? 'danger' : 'secondary'} align="center" style={styles.hint}>
        {wrong ? t('link.scanWrong') : t('link.scanFooter')}
      </Text>
    </Screen>
  )
}

const styles = StyleSheet.create({
  center: { flex: 1, justifyContent: 'center', paddingHorizontal: gutter },
  hint: { marginVertical: space.md },
  camera: { flex: 1, borderRadius: radius.card, overflow: 'hidden' },
  frameWrap: { ...StyleSheet.absoluteFill, alignItems: 'center', justifyContent: 'center' },
  frame: { width: 220, height: 220, borderRadius: radius.card, borderWidth: 3, opacity: 0.9 }
})

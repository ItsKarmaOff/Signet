// App icon choice: grid of the 8 icons (the same as on desktop). Only in the installed app.
import { useState } from 'react'
import { Alert, Platform, StyleSheet, View, useWindowDimensions } from 'react-native'
import { Image } from 'expo-image'
import { APP_ICON_IDS, type AppIconId } from '@signet/shared/mobile/settings'
import { haptic } from '@/lib/haptics'
import { useT } from '@/lib/i18n'
import { useStore } from '@/state'
import { PressableScale, Screen, Text, gutter, icons, space, toast, useColors } from '@/ui'
import { applyAppIcon, appIconSupport } from './appIcon'
import { APP_ICON_IMAGES } from './appIconImages'

const IMAGES = APP_ICON_IMAGES

const COLUMNS = 4
const TILE_GAP = space.md

export function AppIconScreen() {
  const t = useT()
  const c = useColors()
  const current = useStore((s) => s.settings.appIcon)
  const setSettings = useStore((s) => s.setSettings)
  const support = appIconSupport()
  const [busy, setBusy] = useState(false)
  const { width } = useWindowDimensions()
  const cell = Math.floor((width - 2 * gutter - (COLUMNS - 1) * TILE_GAP) / COLUMNS)

  const apply = async (id: AppIconId) => {
    setBusy(true)
    try {
      await applyAppIcon(id)
      setSettings({ appIcon: id })
      haptic('success')
    } catch {
      haptic('error')
      toast.show({ message: t('settings.appIconError') })
    } finally {
      setBusy(false)
    }
  }

  const choose = (id: AppIconId) => {
    if (busy || id === current || support !== 'ready') return
    if (Platform.OS !== 'android') return void apply(id)
    // Android changes the icon by enabling another entry point of the app: the system may close the app. We warn first.
    // If the app is closed before saving, useSyncAppIcon re-reads the actual icon at next launch.
    Alert.alert(t('settings.appIconConfirmTitle'), t('settings.appIconConfirmBody'), [
      { text: t('common.cancel'), style: 'cancel' },
      { text: t('settings.appIconConfirmAction'), onPress: () => void apply(id) }
    ])
  }

  return (
    <Screen scroll contentStyle={{ paddingTop: space.lg, paddingBottom: space.huge, paddingHorizontal: gutter }}>
      {support !== 'ready' ? (
        <Text variant="footnote" color="secondary" style={{ marginBottom: space.lg }}>
          {support === 'unavailable' ? t('settings.appIconExpoGo') : t('settings.appIconUnsupported')}
        </Text>
      ) : null}
      <View style={styles.grid} accessibilityRole="radiogroup">
        {APP_ICON_IDS.map((id) => {
          const selected = id === current
          const name = t(`appIcons.${id}`)
          return (
            <PressableScale
              key={id}
              style={[styles.cell, { width: cell }]}
              disabled={support !== 'ready'}
              onPress={() => choose(id)}
              accessibilityRole="radio"
              accessibilityLabel={name}
              accessibilityState={{ selected, disabled: support !== 'ready' }}
            >
              <View style={[styles.ring, { borderColor: selected ? c.accent : 'transparent' }, support !== 'ready' && styles.dim]}>
                <Image source={IMAGES[id]} style={styles.image} accessibilityIgnoresInvertColors />
                {selected ? (
                  <View style={[styles.badge, { backgroundColor: c.accent, borderColor: c.background }]}>
                    <icons.Check size={13} color="#fff" strokeWidth={3} />
                  </View>
                ) : null}
              </View>
              <Text variant="caption" color={selected ? 'text' : 'secondary'} weight={selected ? '600' : undefined} align="center" numberOfLines={1}>
                {name}
              </Text>
            </PressableScale>
          )
        })}
      </View>
      {support === 'ready' ? (
        <Text variant="footnote" color="secondary" style={{ marginTop: space.lg }}>
          {t(Platform.OS === 'android' ? 'settings.appIconFooterAndroid' : 'settings.appIconFooter')}
        </Text>
      ) : null}
    </Screen>
  )
}

const styles = StyleSheet.create({
  grid: { flexDirection: 'row', flexWrap: 'wrap', columnGap: TILE_GAP, rowGap: space.lg },
  cell: { alignItems: 'center', gap: space.xs },
  // iOS rounds icons to ~22.37% of their side.
  ring: { width: '100%', aspectRatio: 1, borderRadius: 17, borderWidth: 2.5, padding: 2 },
  image: { flex: 1, borderRadius: 13 },
  dim: { opacity: 0.55 },
  badge: {
    position: 'absolute',
    right: -6,
    bottom: -6,
    width: 22,
    height: 22,
    borderRadius: 11,
    borderWidth: 2,
    alignItems: 'center',
    justifyContent: 'center'
  }
})

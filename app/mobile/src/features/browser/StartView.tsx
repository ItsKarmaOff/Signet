// Start screen of an empty tab: search field, the space's favorites and most visited sites.
import { useMemo } from 'react'
import { ScrollView, StyleSheet, View, useWindowDimensions } from 'react-native'
import { LinearGradient } from 'expo-linear-gradient'
import { Image } from 'expo-image'
import { siteOf } from '@signet/shared/site'
import { topSites } from '@signet/shared/mobile/history'
import { DevServers } from '@/features/link'
import { APP_ICON_IMAGES } from '@/features/settings/appIconImages'
import { useT } from '@/lib/i18n'
import { useActiveSpace, usePrivateMode, useStore } from '@/state'
import { Favicon, PressableScale, Text, gutter, icons, radius, space, useColors, useTheme } from '@/ui'

/** App logo at the top of the start page (the same as the desktop icon). */

interface Item {
  key: string
  url: string
  title: string
  favicon?: string
}

interface Props {
  onOpen: (url: string) => void
  /** Space to leave at the bottom for the toolbar. */
  bottomInset: number
  topInset: number
}

const COLUMNS = 4
const GAP = space.md

/** New tab screen. */
export function StartView({ onOpen, bottomInset, topInset }: Props) {
  const t = useT()
  const c = useColors()
  const { gradient, isPrivate } = useTheme()
  const { width } = useWindowDimensions()
  const activeSpace = useActiveSpace()
  const privateMode = usePrivateMode()
  const tabs = useStore((s) => s.workspace.tabs)
  const history = useStore((s) => s.history)
  const appIcon = useStore((s) => s.settings.appIcon)
  const tile = (width - gutter * 2 - GAP * (COLUMNS - 1)) / COLUMNS

  const favorites: Item[] = useMemo(
    () =>
      activeSpace.favorites
        .map((id) => tabs[id])
        .filter((tb) => !!tb && !!(tb.pinnedUrl || tb.url))
        .map((tb) => ({ key: tb.id, url: tb.pinnedUrl || tb.url, title: tb.customTitle || tb.title || siteOf(tb.url), favicon: tb.favicon })),
    [tabs, activeSpace.favorites]
  )
  const frequent: Item[] = useMemo(
    () => (privateMode ? [] : topSites(history, Date.now(), 8).map((h) => ({ key: h.url, url: h.url, title: siteOf(h.url) || h.title, favicon: h.favicon }))),
    [history, privateMode]
  )

  const grid = (items: Item[]) => (
    <View style={styles.grid}>
      {items.map((it) => (
        <PressableScale
          key={it.key}
          accessibilityRole="link"
          accessibilityLabel={it.title}
          haptic="light"
          onPress={() => onOpen(it.url)}
          style={{ width: tile, alignItems: 'center', gap: space.xs }}
        >
          <View style={[styles.icon, { width: tile * 0.78, height: tile * 0.78, backgroundColor: c.surface }]}>
            <Favicon uri={it.favicon} url={it.url} size={Math.round(tile * 0.46)} />
          </View>
          <Text variant="caption" color="secondary" numberOfLines={1} align="center" style={{ width: tile }}>
            {it.title}
          </Text>
        </PressableScale>
      ))}
    </View>
  )

  return (
    <View style={[StyleSheet.absoluteFill, { backgroundColor: c.background }]}>
      <LinearGradient
        colors={gradient.colors as [string, string, ...string[]]}
        start={gradient.start}
        end={gradient.end}
        style={[StyleSheet.absoluteFill, styles.wash]}
      />
      <ScrollView
        contentContainerStyle={{ paddingTop: topInset + space.xxxl, paddingBottom: bottomInset + space.xxl, paddingHorizontal: gutter, gap: space.xxl }}
        keyboardShouldPersistTaps="handled"
        showsVerticalScrollIndicator={false}
      >
        <View style={styles.hero}>
          {isPrivate ? (
            <icons.VenetianMask size={40} color={c.accent} strokeWidth={1.5} />
          ) : (
            <Image source={APP_ICON_IMAGES[appIcon]} style={styles.logo} accessibilityIgnoresInvertColors />
          )}
          <Text variant="title1" align="center">
            {isPrivate ? t('tabs.privateOn') : t('browser.startTitle')}
          </Text>
          {isPrivate ? (
            <Text variant="subhead" color="secondary" align="center">
              {t('tabs.privateHint')}
            </Text>
          ) : null}
        </View>
        <DevServers onOpen={onOpen} />
        {favorites.length > 0 ? (
          <View style={styles.section}>
            <Text variant="footnote" color="secondary" weight="600">
              {t('browser.favorites')}
            </Text>
            {grid(favorites)}
          </View>
        ) : null}
        {frequent.length > 0 ? (
          <View style={styles.section}>
            <Text variant="footnote" color="secondary" weight="600">
              {t('browser.frequent')}
            </Text>
            {grid(frequent)}
          </View>
        ) : null}
      </ScrollView>
    </View>
  )
}

const styles = StyleSheet.create({
  wash: { opacity: 0.35 },
  hero: { alignItems: 'center', gap: space.sm },
  // The icon chosen in Settings › App icon (as on the device's home screen).
  logo: { width: 72, height: 72, borderRadius: 16 },
  section: { gap: space.md },
  grid: { flexDirection: 'row', flexWrap: 'wrap', columnGap: GAP, rowGap: space.lg },
  icon: { borderRadius: radius.group, alignItems: 'center', justifyContent: 'center' }
})

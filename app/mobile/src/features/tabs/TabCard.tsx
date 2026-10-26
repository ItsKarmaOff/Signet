// Tab switcher cards: card with thumbnail (swipe left or right to close) and favorite badge.
import { memo } from 'react'
import { StyleSheet, View, useWindowDimensions } from 'react-native'
import { Image } from 'expo-image'
import { GestureDetector, usePanGesture } from 'react-native-gesture-handler'
import Animated, {
  Easing,
  FadeIn,
  LinearTransition,
  ZoomOut,
  interpolate,
  useAnimatedStyle,
  useSharedValue,
  withSpring,
  withTiming
} from 'react-native-reanimated'
import { scheduleOnRN } from 'react-native-worklets'
import { accentColor, tintColor, withAlpha } from '@signet/shared/mobile/color'
import { siteOf } from '@signet/shared/site'
import type { TabInfo } from '@signet/shared/types'
import { haptic } from '@/lib/haptics'
import { useT } from '@/lib/i18n'
import { Favicon, PressableScale, Text, icons, radius, shadow, space, springs, useColors, useReduceMotion, useTheme } from '@/ui'
import { useThumbnail } from './thumbnails'

export interface TabCardProps {
  tab: TabInfo
  width: number
  selected: boolean
  /** Today's and private tabs close by swiping; pinned and favorites, via the menu. */
  closable: boolean
  onOpen: (id: string) => void
  onClose: (id: string) => void
  onMenu: (id: string) => void
  /** A finger is on the card: horizontal scrolling between spaces pauses. */
  onTouch?: (active: boolean) => void
}

/** Height / width ratio of the thumbnail (a phone page, slightly cropped). */
export const THUMB_RATIO = 1.18
const SWIPE_DISTANCE = 0.38
const SWIPE_VELOCITY = 850
const PAD = 6
const HEADER = 32

function titleOf(tab: TabInfo, fallback: string): string {
  return tab.customTitle || tab.title || siteOf(tab.url) || fallback
}

function TabCardBase({ tab, width, selected, closable, onOpen, onClose, onMenu, onTouch }: TabCardProps) {
  const t = useT()
  const c = useColors()
  const reduce = useReduceMotion()
  const screen = useWindowDimensions().width
  const thumb = useThumbnail(tab.id)
  const tx = useSharedValue(0)
  const label = titleOf(tab, t('browser.newTabTitle'))
  const innerW = width - PAD * 2

  const pan = usePanGesture({
    enabled: closable,
    activeOffsetX: [-12, 12],
    failOffsetY: [-10, 10],
    onBegin: () => {
      'worklet'
      if (onTouch) scheduleOnRN(onTouch, true)
    },
    onUpdate: (e) => {
      'worklet'
      tx.value = e.translationX
    },
    onDeactivate: (e) => {
      'worklet'
      const far = Math.abs(e.translationX) > width * SWIPE_DISTANCE || Math.abs(e.velocityX) > SWIPE_VELOCITY
      if (far) {
        const dir = e.translationX < 0 ? -1 : 1
        tx.value = withTiming(dir * screen, { duration: 180, easing: Easing.out(Easing.quad) }, (finished) => {
          if (finished) scheduleOnRN(onClose, tab.id)
        })
      } else {
        tx.value = withSpring(0, springs.snappy)
      }
    },
    onFinalize: () => {
      'worklet'
      if (onTouch) scheduleOnRN(onTouch, false)
    }
  })

  const swipe = useAnimatedStyle(() => ({
    opacity: interpolate(Math.abs(tx.value), [0, width * 0.9], [1, 0.35], 'clamp'),
    transform: [{ translateX: tx.value }, { rotate: `${interpolate(tx.value, [-width, width], [-6, 6], 'clamp')}deg` }]
  }))

  return (
    <Animated.View
      layout={reduce ? undefined : LinearTransition.springify().damping(26).stiffness(240)}
      entering={reduce ? undefined : FadeIn.duration(160)}
      exiting={reduce ? undefined : ZoomOut.duration(140)}
      style={{ width }}
    >
      <GestureDetector gesture={pan}>
        <Animated.View style={swipe}>
          <PressableScale
            accessibilityRole="button"
            accessibilityLabel={label}
            accessibilityState={{ selected }}
            accessibilityActions={closable ? [{ name: 'activate' }, { name: 'delete', label: t('tabs.close') }] : undefined}
            onAccessibilityAction={(e) => {
              if (e.nativeEvent.actionName === 'delete') onClose(tab.id)
            }}
            scaleTo={0.97}
            onPress={() => onOpen(tab.id)}
            onLongPress={() => {
              haptic('medium')
              onMenu(tab.id)
            }}
            delayLongPress={380}
            style={[styles.card, shadow.card, { width, backgroundColor: c.surface, borderColor: selected ? c.accent : 'transparent' }]}
          >
            <View style={styles.header}>
              <Favicon uri={tab.favicon} url={tab.url} size={18} />
              <Text variant="footnote" weight="600" numberOfLines={1} style={styles.title}>
                {label}
              </Text>
              {closable ? (
                <PressableScale
                  accessibilityRole="button"
                  accessibilityLabel={t('tabs.close')}
                  onPress={() => {
                    haptic('light')
                    onClose(tab.id)
                  }}
                  hitSlop={10}
                  scaleTo={0.85}
                  style={[styles.x, { backgroundColor: c.surface2 }]}
                >
                  <icons.X size={13} color={c.textSecondary} strokeWidth={2.4} />
                </PressableScale>
              ) : tab.pinnedUrl ? (
                <icons.Pin size={13} color={c.textTertiary} strokeWidth={2.2} />
              ) : null}
            </View>
            <View style={[styles.thumb, { width: innerW, height: Math.round(innerW * THUMB_RATIO), backgroundColor: c.surface2 }]}>
              {thumb ? (
                <Image
                  source={{ uri: thumb }}
                  style={StyleSheet.absoluteFill}
                  contentFit="cover"
                  contentPosition="top"
                  transition={reduce ? 0 : 160}
                  accessibilityIgnoresInvertColors
                />
              ) : (
                <Placeholder tab={tab} />
              )}
            </View>
          </PressableScale>
        </Animated.View>
      </GestureDetector>
    </Animated.View>
  )
}

/** Card without thumbnail: site color, large icon, site name (or "New tab"). */
function Placeholder({ tab }: { tab: TabInfo }) {
  const t = useT()
  const c = useColors()
  const { scheme } = useTheme()
  const site = siteOf(tab.url)
  if (!tab.url) {
    return (
      <View style={styles.placeholder}>
        <icons.Plus size={26} color={c.textTertiary} strokeWidth={1.6} />
        <Text variant="caption" color="tertiary">
          {t('browser.newTabTitle')}
        </Text>
      </View>
    )
  }
  const hue = [...site].reduce((h, ch) => (h * 31 + ch.charCodeAt(0)) % 360, 0)
  return (
    <View style={[styles.placeholder, { backgroundColor: withAlpha(tintColor(hue, scheme), 0.14) }]}>
      <Favicon uri={tab.favicon} url={tab.url} size={44} />
      <Text variant="caption" weight="600" numberOfLines={1} color={accentColor(hue, scheme)} style={styles.site}>
        {site}
      </Text>
    </View>
  )
}

export const TabCard = memo(TabCardBase)

export interface FavoriteTileProps {
  tab: TabInfo
  width: number
  selected: boolean
  onOpen: (id: string) => void
  onMenu: (id: string) => void
}

function FavoriteTileBase({ tab, width, selected, onOpen, onMenu }: FavoriteTileProps) {
  const t = useT()
  const c = useColors()
  const reduce = useReduceMotion()
  const label = titleOf(tab, t('browser.newTabTitle'))
  return (
    <Animated.View
      layout={reduce ? undefined : LinearTransition.springify().damping(26).stiffness(240)}
      entering={reduce ? undefined : FadeIn.duration(160)}
      style={{ width }}
    >
      <PressableScale
        accessibilityRole="button"
        accessibilityLabel={label}
        accessibilityState={{ selected }}
        onPress={() => onOpen(tab.id)}
        onLongPress={() => {
          haptic('medium')
          onMenu(tab.id)
        }}
        delayLongPress={380}
        style={styles.fav}
      >
        <View style={[styles.favIcon, shadow.card, { backgroundColor: c.surface, borderColor: selected ? c.accent : 'transparent' }]}>
          <Favicon uri={tab.favicon} url={tab.url} size={32} />
        </View>
        <Text variant="caption2" color="secondary" numberOfLines={1} align="center" style={styles.favLabel}>
          {label}
        </Text>
      </PressableScale>
    </Animated.View>
  )
}

export const FavoriteTile = memo(FavoriteTileBase)

const styles = StyleSheet.create({
  card: { borderRadius: radius.card, borderWidth: 2, padding: PAD - 2 },
  header: { height: HEADER, flexDirection: 'row', alignItems: 'center', gap: space.sm, paddingHorizontal: space.xs },
  title: { flex: 1 },
  x: { width: 22, height: 22, borderRadius: 11, alignItems: 'center', justifyContent: 'center' },
  thumb: { borderRadius: radius.card - PAD - 2, overflow: 'hidden', alignSelf: 'center' },
  placeholder: { flex: 1, alignItems: 'center', justifyContent: 'center', gap: space.sm, padding: space.md },
  site: { maxWidth: '90%' },
  fav: { alignItems: 'center', gap: space.xs },
  favIcon: { width: 60, height: 60, borderRadius: 18, borderWidth: 2, alignItems: 'center', justifyContent: 'center' },
  favLabel: { maxWidth: '100%' }
})

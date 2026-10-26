// Bottom bar: address field (site and padlock at rest; a tap opens the command bar) and buttons.
// It collapses when the page is scrolled down (only the field remains) and comes back when scrolling up.
import { useEffect } from 'react'
import { StyleSheet, View } from 'react-native'
import Animated, { Extrapolation, interpolate, useAnimatedStyle, useSharedValue, withTiming, type SharedValue } from 'react-native-reanimated'
import { siteOf } from '@signet/shared/site'
import type { TabInfo } from '@signet/shared/types'
import { useT } from '@/lib/i18n'
import { GlassBar, IconButton, PressableScale, Text, hit, icons, radius, space, useColors } from '@/ui'

/** Bar heights (excluding the safe area). */
export const PILL_HEIGHT = 44
export const PILL_HEIGHT_COLLAPSED = 34
export const ROW_HEIGHT = 48
export const BAR_PAD_TOP = 8

/** Margin under the field: never below 8, otherwise it tucks against the home indicator. */
export const barBottomPad = (safeBottom: number) => Math.max(safeBottom - 8, 8)

/** Height of the collapsed bar: this is the space the page leaves free at the bottom. */
export const collapsedBarHeight = (safeBottom: number) => BAR_PAD_TOP + PILL_HEIGHT_COLLAPSED + barBottomPad(safeBottom)

interface Props {
  tab: TabInfo
  isPrivate: boolean
  progress: number
  tabCount: number
  /** 0 = full bar, 1 = collapsed. */
  collapsed: SharedValue<number>
  safeBottom: number
  onAddress: () => void
  onCopyAddress: () => void
  onBack: () => void
  onForward: () => void
  onReloadStop: () => void
  onShare: () => void
  onTabs: () => void
  onMenu: () => void
}

export function BottomBar({
  tab,
  isPrivate,
  progress,
  tabCount,
  collapsed,
  safeBottom,
  onAddress,
  onCopyAddress,
  onBack,
  onForward,
  onReloadStop,
  onShare,
  onTabs,
  onMenu
}: Props) {
  const t = useT()
  const c = useColors()
  const site = siteOf(tab.url)
  const secure = tab.url.startsWith('https:')
  const loading = tab.loading && progress < 1

  const pill = useAnimatedStyle(() => ({ height: interpolate(collapsed.value, [0, 1], [PILL_HEIGHT, PILL_HEIGHT_COLLAPSED], Extrapolation.CLAMP) }))
  const row = useAnimatedStyle(() => ({
    height: ROW_HEIGHT * (1 - collapsed.value),
    opacity: interpolate(collapsed.value, [0, 0.6], [1, 0], Extrapolation.CLAMP)
  }))
  const side = useAnimatedStyle(() => ({ opacity: interpolate(collapsed.value, [0, 0.6], [1, 0], Extrapolation.CLAMP) }))

  return (
    <GlassBar edge="top" style={{ paddingTop: BAR_PAD_TOP, paddingBottom: barBottomPad(safeBottom), paddingHorizontal: space.md }}>
      <ProgressLine value={progress} active={loading} />
      <Animated.View style={[styles.pillWrap, pill]}>
        <PressableScale
          onPress={onAddress}
          onLongPress={onCopyAddress}
          scaleTo={0.985}
          accessibilityRole="button"
          accessibilityLabel={`${t('browser.addressPlaceholder')}${site ? `, ${site}` : ''}`}
          accessibilityHint={t('browser.expandBar')}
          style={[styles.pill, { backgroundColor: c.surface2 }]}
        >
          <View style={styles.pillContent} pointerEvents="none">
            {isPrivate ? (
              <icons.VenetianMask size={15} color={c.accent} strokeWidth={2} />
            ) : secure ? (
              <icons.Lock size={13} color={c.textSecondary} strokeWidth={2.2} accessibilityLabel={t('browser.secure')} />
            ) : null}
            <Text variant="subhead" weight="600" numberOfLines={1} color={site ? 'text' : 'tertiary'} style={styles.pillText}>
              {site || t('browser.addressPlaceholder')}
            </Text>
          </View>
        </PressableScale>
        {tab.url ? (
          <Animated.View style={[styles.reload, side]} pointerEvents="box-none">
            <IconButton
              icon={loading ? icons.X : icons.RotateCw}
              size={17}
              tone="muted"
              label={loading ? t('browser.stop') : t('browser.reload')}
              onPress={onReloadStop}
            />
          </Animated.View>
        ) : null}
      </Animated.View>
      <Animated.View style={[styles.row, row]} pointerEvents="box-none">
        <IconButton icon={icons.ChevronLeft} label={t('browser.back')} onPress={onBack} disabled={!tab.canGoBack} />
        <IconButton icon={icons.ChevronRight} label={t('browser.forward')} onPress={onForward} disabled={!tab.canGoForward} />
        <IconButton icon={icons.Share} label={t('browser.share')} onPress={onShare} disabled={!tab.url} />
        <IconButton icon={icons.SquareStack} badge={tabCount} label={t('browser.tabs')} onPress={onTabs} tone={isPrivate ? 'accent' : 'default'} />
        <IconButton icon={icons.Ellipsis} label={t('browser.menu')} onPress={onMenu} />
      </Animated.View>
    </GlassBar>
  )
}

/** Thin progress line at the top edge of the bar. */
function ProgressLine({ value, active }: { value: number; active: boolean }) {
  const c = useColors()
  const width = useSharedValue(0)
  const opacity = useSharedValue(0)
  useEffect(() => {
    if (active) {
      // Restarts from zero when the line was hidden (new load).
      if (opacity.value < 0.05) width.value = 0
      opacity.value = withTiming(1, { duration: 120 })
      width.value = withTiming(Math.max(0.06, value), { duration: 220 })
    } else {
      width.value = withTiming(1, { duration: 140 })
      opacity.value = withTiming(0, { duration: 260 })
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps -- shared values are stable
  }, [value, active])
  const style = useAnimatedStyle(() => ({ width: `${width.value * 100}%`, opacity: opacity.value }))
  return <Animated.View pointerEvents="none" style={[styles.progress, { backgroundColor: c.accent }, style]} />
}

const styles = StyleSheet.create({
  pillWrap: { justifyContent: 'center' },
  pill: { flex: 1, borderRadius: radius.control, justifyContent: 'center', paddingHorizontal: hit },
  pillContent: { flexDirection: 'row', alignItems: 'center', justifyContent: 'center', gap: space.xs },
  pillText: { flexShrink: 1 },
  reload: { position: 'absolute', right: 0, top: 0, bottom: 0, justifyContent: 'center' },
  row: { flexDirection: 'row', alignItems: 'center', justifyContent: 'space-around', overflow: 'hidden' },
  progress: { position: 'absolute', top: 0, left: 0, height: 2.5, borderRadius: 2 }
})

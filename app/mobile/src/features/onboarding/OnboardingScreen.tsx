// First launch: four pages to swipe through (welcome, spaces, privacy, personalization), then `ui.onboardingDone`.
// Motion (parallax, floating, dots) is disabled when "Reduce animations" is on.
import { useEffect, useRef, useState, type ComponentRef } from 'react'
import { useLocalSearchParams } from 'expo-router'
import { ScrollView, StyleSheet, View, useWindowDimensions, type NativeScrollEvent, type NativeSyntheticEvent } from 'react-native'
import Animated, { Extrapolation, interpolate, useAnimatedScrollHandler, useAnimatedStyle, useSharedValue, type SharedValue } from 'react-native-reanimated'
import { LinearGradient } from 'expo-linear-gradient'
import { useSafeAreaInsets } from 'react-native-safe-area-context'
import { withAlpha } from '@signet/shared/mobile/color'
import { SEARCH_ENGINES } from '@signet/shared/mobile/search'
import type { Theme } from '@signet/shared/types'
import { haptic } from '@/lib/haptics'
import { useT } from '@/lib/i18n'
import { useStore } from '@/state'
import { Button, Favicon, PressableScale, SegmentedControl, Text, gutter, icons, radius, space, useReduceMotion, useTheme } from '@/ui'
import { PersonalizeArt, PrivacyArt, SpacesArt, WelcomeArt } from './Illustrations'

const PAGES = ['welcome', 'spaces', 'privacy', 'personalize'] as const
type PageKey = (typeof PAGES)[number]

export function OnboardingScreen() {
  const t = useT()
  const { colors, gradient } = useTheme()
  const insets = useSafeAreaInsets()
  const { width } = useWindowDimensions()
  const reduce = useReduceMotion()
  const setOnboardingDone = useStore((s) => s.setOnboardingDone)
  const scroller = useRef<ComponentRef<typeof ScrollView>>(null)
  const [page, setPage] = useState(0)
  const scrollX = useSharedValue(0)
  const onScroll = useAnimatedScrollHandler((e) => {
    scrollX.value = e.contentOffset.x
  })
  const last = page === PAGES.length - 1
  // DEV: `/onboarding?devPage=2` jumps to a page (acceptance testing in the simulator).
  const { devPage } = useLocalSearchParams<{ devPage?: string }>()
  useEffect(() => {
    if (!__DEV__ || devPage === undefined) return
    const n = Math.min(PAGES.length - 1, Math.max(0, Number(devPage) || 0))
    const id = setTimeout(() => {
      scroller.current?.scrollTo({ x: n * width, animated: false })
      setPage(n)
    }, 400)
    return () => clearTimeout(id)
  }, [devPage, width])

  const finish = () => {
    haptic('success')
    setOnboardingDone()
  }
  const next = () => {
    if (last) return finish()
    haptic('selection')
    scroller.current?.scrollTo({ x: (page + 1) * width, animated: !reduce })
    if (reduce) setPage(page + 1)
  }
  const onEnd = (e: NativeSyntheticEvent<NativeScrollEvent>) => setPage(Math.round(e.nativeEvent.contentOffset.x / width))

  return (
    <View style={[styles.fill, { backgroundColor: colors.background }]}>
      <LinearGradient
        colors={gradient.colors.map((x) => withAlpha(x, 0.35)) as [string, string, ...string[]]}
        start={gradient.start}
        end={gradient.end}
        style={StyleSheet.absoluteFill}
      />
      <View style={[styles.top, { paddingTop: insets.top + space.sm }]}>
        {last ? null : <Button title={t('onboarding.skip')} variant="plain" onPress={finish} />}
      </View>

      <Animated.ScrollView
        ref={scroller as never}
        horizontal
        pagingEnabled
        showsHorizontalScrollIndicator={false}
        scrollEventThrottle={16}
        onScroll={onScroll}
        onMomentumScrollEnd={onEnd}
        style={styles.fill}
        accessibilityLabel={t('onboarding.pageOf', { current: page + 1, total: PAGES.length })}
      >
        {PAGES.map((key, index) => (
          <Page key={key} kind={key} index={index} width={width} scrollX={scrollX} reduce={reduce} />
        ))}
      </Animated.ScrollView>

      <View style={[styles.bottom, { paddingBottom: insets.bottom + space.xl }]}>
        <View style={styles.dots} accessibilityElementsHidden importantForAccessibility="no-hide-descendants">
          {PAGES.map((key, index) => (
            <Dot key={key} index={index} width={width} scrollX={scrollX} reduce={reduce} active={page === index} />
          ))}
        </View>
        <Button title={last ? t('onboarding.start') : t('onboarding.next')} size="lg" fullWidth onPress={next} />
      </View>
    </View>
  )
}

function Page({ kind, index, width, scrollX, reduce }: { kind: PageKey; index: number; width: number; scrollX: SharedValue<number>; reduce: boolean }) {
  const t = useT()
  // The illustration scrolls slightly slower than the text, and fades out as it moves away.
  const art = useAnimatedStyle(() => {
    if (reduce) return {}
    const d = (scrollX.value - index * width) / width
    return {
      opacity: interpolate(Math.abs(d), [0, 1], [1, 0], Extrapolation.CLAMP),
      transform: [{ translateX: d * width * 0.3 }, { scale: interpolate(Math.abs(d), [0, 1], [1, 0.9], Extrapolation.CLAMP) }]
    }
  })
  return (
    <View style={[styles.page, { width }]}>
      <Animated.View style={art}>
        {kind === 'welcome' ? <WelcomeArt /> : kind === 'spaces' ? <SpacesArt /> : kind === 'privacy' ? <PrivacyArt /> : <PersonalizeArt />}
      </Animated.View>
      <View style={styles.texts}>
        <Text variant="title1" align="center" accessibilityRole="header">
          {t(`onboarding.${kind}Title`)}
        </Text>
        <Text variant="body" color="secondary" align="center">
          {t(`onboarding.${kind}Body`)}
        </Text>
      </View>
      {kind === 'personalize' ? <Personalize /> : null}
    </View>
  )
}

function Personalize() {
  const t = useT()
  const { colors } = useTheme()
  const engine = useStore((s) => s.settings.searchEngine)
  const theme = useStore((s) => s.settings.theme)
  const setSettings = useStore((s) => s.setSettings)
  return (
    <View style={styles.choices}>
      <View style={styles.grid}>
        {SEARCH_ENGINES.map((e) => {
          const selected = e.id === engine
          return (
            <PressableScale
              key={e.id}
              accessibilityRole="radio"
              accessibilityState={{ selected }}
              accessibilityLabel={e.name}
              haptic="selection"
              onPress={() => setSettings({ searchEngine: e.id })}
              style={[styles.pill, { backgroundColor: colors.surface, borderColor: selected ? colors.accent : 'transparent' }]}
            >
              <Favicon uri={`https://icons.duckduckgo.com/ip3/${new URL(e.template).hostname}.ico`} url={e.template} size={20} />
              <Text variant="subhead" weight={selected ? '600' : '400'} numberOfLines={1}>
                {e.name}
              </Text>
            </PressableScale>
          )
        })}
      </View>
      <SegmentedControl<Theme>
        options={[
          { value: 'system', label: t('settings.themeSystem'), icon: icons.Monitor },
          { value: 'light', label: t('settings.themeLight'), icon: icons.Sun },
          { value: 'dark', label: t('settings.themeDark'), icon: icons.Moon }
        ]}
        value={theme}
        onChange={(next) => setSettings({ theme: next })}
      />
    </View>
  )
}

function Dot({ index, width, scrollX, reduce, active }: { index: number; width: number; scrollX: SharedValue<number>; reduce: boolean; active: boolean }) {
  const { colors } = useTheme()
  const style = useAnimatedStyle(() => {
    if (reduce) return { width: active ? 22 : 8, opacity: active ? 1 : 0.35 }
    const near = interpolate(scrollX.value / width, [index - 1, index, index + 1], [0, 1, 0], Extrapolation.CLAMP)
    return { width: 8 + near * 14, opacity: 0.35 + near * 0.65 }
  })
  return <Animated.View style={[styles.dot, { backgroundColor: colors.accent }, style]} />
}

const styles = StyleSheet.create({
  fill: { flex: 1 },
  top: { alignItems: 'flex-end', paddingHorizontal: gutter, minHeight: 56 },
  page: { flex: 1, paddingHorizontal: gutter + 8, justifyContent: 'center', gap: space.xxl },
  texts: { gap: space.md },
  bottom: { paddingHorizontal: gutter + 8, gap: space.xl },
  dots: { flexDirection: 'row', justifyContent: 'center', gap: space.sm },
  dot: { height: 8, borderRadius: 4 },
  choices: { gap: space.lg },
  grid: { flexDirection: 'row', flexWrap: 'wrap', gap: space.sm },
  pill: {
    flexBasis: '48%',
    flexGrow: 1,
    minHeight: 44,
    flexDirection: 'row',
    alignItems: 'center',
    gap: space.sm,
    paddingHorizontal: space.md,
    borderRadius: radius.control,
    borderWidth: 1.5
  }
})

// Tab switcher (full-screen modal): spaces at the top (row + horizontal scrolling between spaces), grid of cards with
// thumbnails, favorites / pinned / today, and at the bottom the Private toggle, "+", "Close today's" and "OK".
import { useCallback, useEffect, useMemo, useRef, useState, type ComponentRef } from 'react'
import { Alert, ScrollView, StyleSheet, View, useWindowDimensions, type NativeScrollEvent, type NativeSyntheticEvent } from 'react-native'
import { LinearGradient } from 'expo-linear-gradient'
import { useRouter } from 'expo-router'
import Animated, { FadeIn, useAnimatedStyle, useSharedValue, withTiming } from 'react-native-reanimated'
import { useSafeAreaInsets } from 'react-native-safe-area-context'
import { withAlpha } from '@signet/shared/mobile/color'
import { canAddSpace, tabsOf } from '@signet/shared/mobile/workspace'
import { haptic } from '@/lib/haptics'
import { useT } from '@/lib/i18n'
import { useStore } from '@/state'
import {
  Button,
  GlassBar,
  IconButton,
  PressableScale,
  SegmentedControl,
  Text,
  ToastHost,
  gutter,
  icons,
  radius,
  space,
  toast,
  useReduceMotion,
  useTheme
} from '@/ui'
import { SpaceStrip } from '../spaces'
import { PrivateLockOverlay, requestPrivateAccess } from './privateLock'
import { TabMenu } from './TabMenu'
import { TabSections } from './TabSections'

export function TabsScreen() {
  const t = useT()
  const router = useRouter()
  const insets = useSafeAreaInsets()
  const { colors: c, gradient, scheme } = useTheme()
  const reduce = useReduceMotion()
  const { width } = useWindowDimensions()
  const workspace = useStore((s) => s.workspace)
  const isPrivate = workspace.privateMode
  const { spaces, activeSpaceId } = workspace
  const index = Math.max(
    0,
    spaces.findIndex((s) => s.id === activeSpaceId)
  )
  const active = spaces[index]
  const [barH, setBarH] = useState(170)
  const [pagerH, setPagerH] = useState(600)
  const [pagerLocked, setPagerLocked] = useState(false)
  const [menuId, setMenuId] = useState<string | null>(null)
  const pager = useRef<ComponentRef<typeof ScrollView>>(null)
  const leave = useSharedValue(0)
  const leaveStyle = useAnimatedStyle(() => ({ opacity: 1 - leave.value, transform: [{ scale: 1 + leave.value * 0.05 }] }))

  // Scrolling follows the active space (space row, swipe on the browser bar…).
  useEffect(() => {
    pager.current?.scrollTo({ x: index * width, animated: true })
  }, [index, width])

  const todayCount = active?.today.length ?? 0
  const privateCount = workspace.priv.tabIds.length

  const dismiss = useCallback(() => router.dismiss(), [router])

  const open = useCallback(
    (id: string) => {
      haptic('light')
      useStore.getState().activateTab(id)
      if (reduce) return dismiss()
      leave.value = withTiming(1, { duration: 130 })
      setTimeout(dismiss, 120)
    },
    [dismiss, leave, reduce]
  )

  const close = useCallback(
    (id: string) => {
      const state = useStore.getState()
      const wasPrivate = state.workspace.priv.tabIds.includes(id)
      state.closeTab(id)
      if (!wasPrivate) toast.show({ message: t('tabs.closed'), actionLabel: t('common.undo'), onAction: () => useStore.getState().undoCloseTab() })
    },
    [t]
  )

  const newTab = () => {
    haptic('light')
    router.push('/command?newTab=1')
  }

  const closeToday = () => {
    const count = todayCount
    if (count === 0) return
    Alert.alert(t('tabs.closeTodayTitle'), t('tabs.closeTodayMessage', { count }), [
      { text: t('common.cancel'), style: 'cancel' },
      {
        text: t('tabs.closeToday'),
        style: 'destructive',
        onPress: () => {
          useStore.getState().closeTodayTabs(activeSpaceId)
          haptic('success')
          toast.show({ message: t('tabs.closeTodayDone', { count }), actionLabel: t('common.undo'), onAction: () => useStore.getState().undoCloseTodayTabs() })
        }
      }
    ])
  }

  const closePrivate = () => {
    Alert.alert(t('tabs.closePrivateTitle'), t('tabs.closePrivateMessage'), [
      { text: t('common.cancel'), style: 'cancel' },
      {
        text: t('tabs.closePrivate'),
        style: 'destructive',
        onPress: () => {
          // Private WebViews are unmounted with their tabs: their non-persistent storage is discarded with them.
          useStore.getState().closePrivateTabs()
          haptic('success')
          toast.show({ message: t('tabs.closePrivateDone') })
        }
      }
    ])
  }

  const setMode = async (mode: 'normal' | 'private') => {
    if (mode === 'private') {
      if (await requestPrivateAccess()) useStore.getState().setPrivateMode(true)
    } else useStore.getState().setPrivateMode(false)
  }

  const editSpace = (id: string) => router.push({ pathname: '/spaces/edit/[id]', params: { id } })

  const onPagerEnd = (e: NativeSyntheticEvent<NativeScrollEvent>) => {
    const i = Math.round(e.nativeEvent.contentOffset.x / width)
    const next = spaces[Math.min(Math.max(0, i), spaces.length - 1)]
    if (next && next.id !== activeSpaceId) {
      haptic('selection')
      useStore.getState().setActiveSpace(next.id)
    }
  }

  const common = { width, bottomInset: barH + space.xl, onOpen: open, onClose: close, onMenu: setMenuId, onTouch: setPagerLocked }
  const privateTabs = useMemo(() => tabsOf(workspace, workspace.priv.tabIds), [workspace])

  return (
    <View style={[styles.fill, { backgroundColor: c.background }]}>
      <LinearGradient
        pointerEvents="none"
        colors={gradient.colors.map((col) => withAlpha(col, scheme === 'dark' ? 0.3 : 0.24)) as [string, string, ...string[]]}
        start={gradient.start}
        end={gradient.end}
        style={StyleSheet.absoluteFill}
      />
      <Animated.View style={[styles.fill, leaveStyle]}>
        <View style={{ paddingTop: insets.top + space.sm, paddingBottom: space.xs }}>
          {isPrivate ? (
            <View style={styles.privateHead}>
              <icons.VenetianMask size={22} color={c.accent} strokeWidth={1.8} />
              <View style={styles.privateText}>
                <Text variant="headline">{t('tabs.privateOn')}</Text>
                <Text variant="footnote" color="secondary" numberOfLines={2}>
                  {t('tabs.privateHint')}
                </Text>
              </View>
            </View>
          ) : (
            <SpaceStrip
              spaces={spaces}
              activeId={activeSpaceId}
              onSelect={(id) => useStore.getState().setActiveSpace(id)}
              onEdit={editSpace}
              onAdd={canAddSpace(workspace) ? () => router.push({ pathname: '/spaces/edit/[id]', params: { id: 'new' } }) : undefined}
            />
          )}
        </View>

        {isPrivate ? (
          <Animated.View key="private" entering={reduce ? undefined : FadeIn.duration(180)} style={styles.fill}>
            <TabSections {...common} favorites={[]} pinned={[]} today={privateTabs} selectedId={workspace.priv.activeId} />
          </Animated.View>
        ) : (
          <Animated.View key="normal" entering={reduce ? undefined : FadeIn.duration(180)} style={styles.fill}>
            <ScrollView
              ref={pager}
              horizontal
              pagingEnabled
              bounces={false}
              scrollEnabled={!pagerLocked && spaces.length > 1}
              showsHorizontalScrollIndicator={false}
              contentOffset={{ x: index * width, y: 0 }}
              onMomentumScrollEnd={onPagerEnd}
              onLayout={(e) => setPagerH(e.nativeEvent.layout.height)}
              style={styles.fill}
            >
              {spaces.map((s, i) =>
                // Only the displayed page and its neighbors are rendered (up to 12 spaces).
                Math.abs(i - index) <= 1 ? (
                  <TabSections
                    key={s.id}
                    {...common}
                    height={pagerH}
                    favorites={tabsOf(workspace, s.favorites)}
                    pinned={tabsOf(workspace, s.pinned)}
                    today={tabsOf(workspace, s.today)}
                    selectedId={i === index ? s.activeTabId : null}
                  />
                ) : (
                  <View key={s.id} style={{ width }} />
                )
              )}
            </ScrollView>
          </Animated.View>
        )}

        <GlassBar edge="top" style={[styles.bar, { paddingBottom: Math.max(insets.bottom, space.md) }]}>
          <View onLayout={(e) => setBarH(e.nativeEvent.layout.height)}>
            <View style={styles.segment}>
              <SegmentedControl
                options={[
                  { value: 'normal', label: t('tabs.normal'), icon: icons.SquareStack },
                  { value: 'private', label: t('tabs.private'), icon: icons.VenetianMask }
                ]}
                value={isPrivate ? 'private' : 'normal'}
                onChange={(v) => void setMode(v)}
              />
            </View>
            <View style={styles.actions}>
              <View style={styles.side}>
                <IconButton
                  icon={icons.Trash}
                  label={isPrivate ? t('tabs.closePrivate') : t('tabs.closeToday')}
                  tone={isPrivate || todayCount > 0 ? 'default' : 'muted'}
                  disabled={isPrivate ? privateCount === 0 : todayCount === 0}
                  onPress={isPrivate ? closePrivate : closeToday}
                />
              </View>
              <PressableScale
                accessibilityRole="button"
                accessibilityLabel={isPrivate ? t('tabs.newPrivate') : t('tabs.new')}
                haptic="light"
                onPress={newTab}
                scaleTo={0.9}
                style={[styles.plus, { backgroundColor: c.accent }]}
              >
                <icons.Plus size={26} color={c.onAccent} strokeWidth={2.4} />
              </PressableScale>
              <View style={[styles.side, styles.sideEnd]}>
                <Button title={t('tabs.done')} variant="plain" onPress={dismiss} style={styles.done} />
              </View>
            </View>
          </View>
        </GlassBar>
      </Animated.View>

      <ToastHost bottomOffset={barH - insets.bottom + space.sm} />
      <TabMenu
        tabId={menuId}
        workspace={workspace}
        onDismiss={() => setMenuId(null)}
        onPin={(id, pinned) => useStore.getState().pinTab(id, pinned)}
        onFavorite={(id, favorite) => useStore.getState().favoriteTab(id, favorite)}
        onDuplicate={(id) => useStore.getState().duplicateTab(id)}
        onMove={(id, spaceId) => useStore.getState().moveTabToSpace(id, spaceId)}
        onCloseTab={close}
      />
      <PrivateLockOverlay />
    </View>
  )
}

const styles = StyleSheet.create({
  fill: { flex: 1 },
  privateHead: { flexDirection: 'row', alignItems: 'center', gap: space.md, paddingHorizontal: gutter, minHeight: 38 },
  privateText: { flex: 1 },
  bar: { position: 'absolute', left: 0, right: 0, bottom: 0, paddingTop: space.sm },
  segment: { paddingHorizontal: gutter, paddingBottom: space.sm },
  actions: { flexDirection: 'row', alignItems: 'center', paddingHorizontal: space.sm },
  side: { flex: 1, alignItems: 'flex-start' },
  sideEnd: { alignItems: 'flex-end' },
  done: { alignSelf: 'flex-end' },
  plus: { width: 56, height: 44, borderRadius: radius.pill, alignItems: 'center', justifyContent: 'center' }
})

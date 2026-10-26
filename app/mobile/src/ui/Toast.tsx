// Toast: brief message at the bottom of the screen, with an optional action ("Undo" after closing a tab).
// One at a time. `toast.show(...)` can be called from anywhere; <ToastHost /> is mounted once in app/_layout.tsx.
import { useEffect, useRef } from 'react'
import { StyleSheet, View } from 'react-native'
import Animated, { useAnimatedStyle, useSharedValue, withTiming } from 'react-native-reanimated'
import { useSafeAreaInsets } from 'react-native-safe-area-context'
import { create } from 'zustand'
import { haptic } from '@/lib/haptics'
import { GlassBar } from './GlassBar'
import { PressableScale, springs, useReduceMotion, useSpringTo } from './motion'
import { useColors } from './theme'
import { Text } from './Text'
import { radius, space } from './tokens'

export interface ToastOptions {
  message: string
  actionLabel?: string
  onAction?: () => void
  /** Display duration in ms (default 4000). */
  duration?: number
}

interface ToastState {
  current: (ToastOptions & { id: number }) | null
}

let counter = 0
const useToastStore = create<ToastState>(() => ({ current: null }))

// Several hosts can be mounted (root, then one per modal covering the root): only the last mounted one, the one visible,
// shows the toast, vibrates and handles the delay. The others render nothing.
let hostCounter = 0
const useHosts = create<{ ids: number[] }>(() => ({ ids: [] }))

// The delay is global, not owned by a host: when changing screens, the host that showed the toast disappears (or is no longer
// the topmost one) and its timer with it; the toast would then stay displayed indefinitely, from screen to screen.
let hideTimer: ReturnType<typeof setTimeout> | null = null
let lastHaptic = 0

export const toast = {
  show(options: ToastOptions): void {
    const id = ++counter
    useToastStore.setState({ current: { ...options, id } })
    if (hideTimer) clearTimeout(hideTimer)
    hideTimer = setTimeout(() => {
      hideTimer = null
      if (useToastStore.getState().current?.id === id) useToastStore.setState({ current: null })
    }, options.duration ?? 4000)
  },
  hide(): void {
    if (hideTimer) clearTimeout(hideTimer)
    hideTimer = null
    useToastStore.setState({ current: null })
  }
}

/** Toast host: mount once, at the very top of the tree. `bottomOffset`: distance above the safe area (address bar). */
export function ToastHost({ bottomOffset = 104 }: { bottomOffset?: number }) {
  const stored = useToastStore((s) => s.current)
  const hostId = useRef(++hostCounter).current
  const isTop = useHosts((s) => s.ids[s.ids.length - 1] === hostId)
  useEffect(() => {
    useHosts.setState((s) => ({ ids: [...s.ids, hostId] }))
    return () => useHosts.setState((s) => ({ ids: s.ids.filter((id) => id !== hostId) }))
  }, [hostId])
  const current = isTop ? stored : null
  const insets = useSafeAreaInsets()
  const c = useColors()
  const reduce = useReduceMotion()
  const spring = useSpringTo(reduce)
  const shown = useSharedValue(0)
  const style = useAnimatedStyle(() => ({ opacity: shown.value, transform: [{ translateY: (1 - shown.value) * 24 }] }))

  useEffect(() => {
    if (!current) {
      shown.value = withTiming(0, { duration: reduce ? 0 : 160 })
      return
    }
    // A single vibration per toast, even if it moves from one host to another when changing screens.
    if (lastHaptic !== current.id) {
      lastHaptic = current.id
      haptic('light')
    }
    shown.value = spring(1, springs.default)
    // eslint-disable-next-line react-hooks/exhaustive-deps -- only the new value of `current` restarts the animation
  }, [current?.id])

  if (!current) return null
  return (
    <Animated.View pointerEvents="box-none" style={[styles.host, { bottom: insets.bottom + bottomOffset }, style]}>
      <GlassBar floating style={styles.bar}>
        <View style={styles.row} accessibilityLiveRegion="polite">
          <Text variant="subhead" style={styles.message} numberOfLines={2}>
            {current.message}
          </Text>
          {current.actionLabel ? (
            <PressableScale
              accessibilityRole="button"
              onPress={() => {
                current.onAction?.()
                toast.hide()
              }}
              hitSlop={8}
              style={styles.action}
            >
              <Text variant="subhead" weight="600" style={{ color: c.accent }}>
                {current.actionLabel}
              </Text>
            </PressableScale>
          ) : null}
        </View>
      </GlassBar>
    </Animated.View>
  )
}

const styles = StyleSheet.create({
  host: { position: 'absolute', left: space.lg, right: space.lg, alignItems: 'center' },
  bar: { alignSelf: 'stretch', maxWidth: 480, borderRadius: radius.group + 6 },
  row: { flexDirection: 'row', alignItems: 'center', paddingVertical: space.md, paddingHorizontal: space.lg, gap: space.md },
  message: { flex: 1 },
  action: { minHeight: 32, justifyContent: 'center' }
})

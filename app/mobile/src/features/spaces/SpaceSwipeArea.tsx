// Swipe area between spaces: wraps the browser's bottom bar. Swiping left opens the next space,
// right opens the previous one (spring + haptics), and a badge names the destination space for a brief moment.
// At the edge (first or last space) the bar resists then snaps back, with a very light haptic tick.
import { useEffect, useRef, useState, type ReactNode } from 'react'
import { StyleSheet, type StyleProp, type ViewStyle } from 'react-native'
import { GestureDetector, usePanGesture } from 'react-native-gesture-handler'
import Animated, { useAnimatedStyle, useSharedValue, withDelay, withSequence, withSpring, withTiming } from 'react-native-reanimated'
import { scheduleOnRN } from 'react-native-worklets'
import { adjacentSpace } from '@signet/shared/mobile/workspace'
import { haptic } from '@/lib/haptics'
import { captureCurrentThumbnail } from '@/lib/liveCapture'
import { useStore } from '@/state'
import { GlassBar, Text, radius, space, springs, useReduceMotion } from '@/ui'

export interface SpaceSwipeAreaProps {
  children?: ReactNode
  style?: StyleProp<ViewStyle>
}

/** Distance (pt) or velocity (pt/s) beyond which a swipe changes space. */
const DISTANCE = 56
const VELOCITY = 650

export function SpaceSwipeArea({ children, style }: SpaceSwipeAreaProps) {
  const reduce = useReduceMotion()
  const tx = useSharedValue(0)
  const label = useSharedValue(0)
  const [shown, setShown] = useState<{ icon: string; name: string } | null>(null)
  const clear = useRef<ReturnType<typeof setTimeout> | null>(null)
  useEffect(() => () => (clear.current ? clearTimeout(clear.current) : undefined), [])

  /** Changes space; returns false at the edge. No effect in private browsing (no visible active space). */
  const switchBy = (delta: 1 | -1) => {
    const state = useStore.getState()
    if (state.workspace.privateMode) return
    const next = adjacentSpace(state.workspace, delta)
    if (!next) {
      haptic('light')
      return
    }
    void captureCurrentThumbnail().then(() => useStore.getState().setActiveSpace(next.id))
    haptic('selection')
    setShown({ icon: next.icon, name: next.name })
    label.value = withSequence(withTiming(1, { duration: 140 }), withDelay(900, withTiming(0, { duration: 260 })))
    if (clear.current) clearTimeout(clear.current)
    clear.current = setTimeout(() => setShown(null), 1500)
  }

  const pan = usePanGesture({
    activeOffsetX: [-14, 14],
    failOffsetY: [-12, 12],
    onUpdate: (e) => {
      'worklet'
      if (!reduce) tx.value = e.translationX * 0.3
    },
    onDeactivate: (e) => {
      'worklet'
      if (Math.abs(e.translationX) > DISTANCE || Math.abs(e.velocityX) > VELOCITY) scheduleOnRN(switchBy, e.translationX < 0 ? 1 : -1)
      tx.value = withSpring(0, springs.snappy)
    }
  })

  const moved = useAnimatedStyle(() => ({ transform: [{ translateX: tx.value }] }))
  const pill = useAnimatedStyle(() => ({ opacity: label.value, transform: [{ translateY: (1 - label.value) * 8 }, { scale: 0.96 + label.value * 0.04 }] }))

  return (
    <GestureDetector gesture={pan}>
      <Animated.View style={[style, moved]}>
        {children}
        {shown ? (
          <Animated.View pointerEvents="none" style={[styles.pillHost, pill]}>
            <GlassBar floating style={styles.pill}>
              <Text variant="subhead" weight="600" numberOfLines={1} style={styles.pillText}>
                {`${shown.icon}  ${shown.name}`}
              </Text>
            </GlassBar>
          </Animated.View>
        ) : null}
      </Animated.View>
    </GestureDetector>
  )
}

const styles = StyleSheet.create({
  pillHost: { position: 'absolute', left: 0, right: 0, bottom: '100%', alignItems: 'center', paddingBottom: space.md },
  pill: { borderRadius: radius.pill, maxWidth: '80%' },
  pillText: { paddingHorizontal: space.lg, paddingVertical: space.sm }
})

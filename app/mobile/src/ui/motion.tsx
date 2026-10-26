// Motion: Apple-style springs (damped, no pronounced bounce), control press feedback, reduced motion respected.
import { useEffect, useState, type ReactNode } from 'react'
import { AccessibilityInfo, Pressable, type GestureResponderEvent, type PressableProps, type StyleProp, type ViewStyle } from 'react-native'
import Animated, { useAnimatedStyle, useSharedValue, withSpring, withTiming, type WithSpringConfig } from 'react-native-reanimated'
import { haptic, type HapticKind } from '@/lib/haptics'

/**
 * App springs. `default` for most transitions, `snappy` for gestures, `gentle` for large surfaces,
 * `sheet` for a panel rising from the bottom: critically damped (ratio ≈ 1), no bounce, like iOS sheets.
 * (`default` bounces a little, barely visible over a short distance but very visible over the full screen height.)
 */
export const springs = {
  default: { damping: 24, stiffness: 260, mass: 1 },
  sheet: { damping: 34, stiffness: 300, mass: 1 },
  snappy: { damping: 22, stiffness: 320, mass: 0.9 },
  gentle: { damping: 26, stiffness: 200, mass: 1 },
  press: { damping: 20, stiffness: 400, mass: 0.6 }
} as const satisfies Record<string, WithSpringConfig>

/** Scale of a pressed control. */
export const PRESS_SCALE = 0.96

/** True when the user asked to reduce animations (system accessibility setting). */
export function useReduceMotion(): boolean {
  const [reduce, setReduce] = useState(false)
  useEffect(() => {
    let alive = true
    void AccessibilityInfo.isReduceMotionEnabled().then((v) => alive && setReduce(v))
    const sub = AccessibilityInfo.addEventListener('reduceMotionChanged', setReduce)
    return () => {
      alive = false
      sub.remove()
    }
  }, [])
  return reduce
}

/** True if "Reduce transparency" is on (iOS): glass bars become opaque. */
export function useReduceTransparency(): boolean {
  const [reduce, setReduce] = useState(false)
  useEffect(() => {
    let alive = true
    void AccessibilityInfo.isReduceTransparencyEnabled().then((v) => alive && setReduce(v))
    const sub = AccessibilityInfo.addEventListener('reduceTransparencyChanged', setReduce)
    return () => {
      alive = false
      sub.remove()
    }
  }, [])
  return reduce
}

/** Spring, or immediate change when motion is reduced. */
export function useSpringTo(reduceMotion: boolean) {
  return (value: number, config: WithSpringConfig = springs.default) => (reduceMotion ? withTiming(value, { duration: 0 }) : withSpring(value, config))
}

const APressable = Animated.createAnimatedComponent(Pressable)

export interface PressableScaleProps extends Omit<PressableProps, 'style' | 'children'> {
  children?: ReactNode
  style?: StyleProp<ViewStyle>
  /** Pressed scale (default 0.96). */
  scaleTo?: number
  /** Haptic feedback on touch (default: none). */
  haptic?: HapticKind
}

/** Pressable that sinks under the finger (spring); with reduced motion, only opacity changes. */
export function PressableScale({
  children,
  style,
  scaleTo = PRESS_SCALE,
  haptic: kind,
  onPressIn,
  onPressOut,
  onPress,
  disabled,
  ...rest
}: PressableScaleProps) {
  const reduce = useReduceMotion()
  const scale = useSharedValue(1)
  const dim = useSharedValue(1)
  const animated = useAnimatedStyle(() => ({ transform: [{ scale: scale.value }], opacity: dim.value }))
  const press = (down: boolean) => {
    if (reduce) dim.value = withTiming(down ? 0.6 : 1, { duration: 80 })
    else scale.value = withSpring(down ? scaleTo : 1, springs.press)
  }
  return (
    <APressable
      {...rest}
      disabled={disabled}
      onPressIn={(e: GestureResponderEvent) => {
        press(true)
        onPressIn?.(e)
      }}
      onPressOut={(e: GestureResponderEvent) => {
        press(false)
        onPressOut?.(e)
      }}
      onPress={(e: GestureResponderEvent) => {
        if (kind) haptic(kind)
        onPress?.(e)
      }}
      style={[style, animated]}
    >
      {children}
    </APressable>
  )
}

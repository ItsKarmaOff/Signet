import { useEffect, useState } from 'react'
import { StyleSheet, View, type LayoutChangeEvent } from 'react-native'
import { LinearGradient } from 'expo-linear-gradient'
import { GestureDetector, usePanGesture } from 'react-native-gesture-handler'
import Animated, { useAnimatedStyle, useSharedValue } from 'react-native-reanimated'
import { scheduleOnRN } from 'react-native-worklets'
import { hslToRgb, rgbToHex } from '@signet/shared/mobile/color'
import { haptic } from '@/lib/haptics'
import { shadow, useColors } from '@/ui'

const STOPS = Array.from({ length: 13 }, (_, i) => rgbToHex(hslToRgb(i * 30, 0.7, 0.62)))
const KNOB = 28

export interface HueSliderProps {
  value: number
  onChange: (hue: number) => void
  label: string
}

/** Hue slider 0–359 over the space color gradient (the same hues as the desktop picker). */
export function HueSlider({ value, onChange, label }: HueSliderProps) {
  const c = useColors()
  const [width, setWidth] = useState(0)
  const span = Math.max(1, width - KNOB)
  const hue = useSharedValue(value)
  const tick = (h: number) => {
    onChange(h)
  }
  const set = (x: number) => {
    'worklet'
    const next = Math.min(359, Math.max(0, Math.round(((x - KNOB / 2) / span) * 359)))
    if (next !== hue.value) {
      hue.value = next
      scheduleOnRN(tick, next)
    }
  }
  const pan = usePanGesture({
    activeOffsetX: [-3, 3],
    failOffsetY: [-14, 14],
    onBegin: (e) => {
      'worklet'
      set(e.x)
      scheduleOnRN(haptic, 'selection')
    },
    onUpdate: (e) => {
      'worklet'
      set(e.x)
    }
  })
  // The value can also change from outside (color swatches).
  useEffect(() => {
    hue.value = value
  }, [hue, value])
  const knob = useAnimatedStyle(() => ({ transform: [{ translateX: (hue.value / 359) * span }] }))
  const color = rgbToHex(hslToRgb(value, 0.7, 0.62))
  return (
    <GestureDetector gesture={pan}>
      <View
        style={styles.hit}
        onLayout={(e: LayoutChangeEvent) => setWidth(e.nativeEvent.layout.width)}
        accessible
        accessibilityRole="adjustable"
        accessibilityLabel={label}
        accessibilityValue={{ min: 0, max: 359, now: value }}
        accessibilityActions={[{ name: 'increment' }, { name: 'decrement' }]}
        onAccessibilityAction={(e) => onChange((value + (e.nativeEvent.actionName === 'increment' ? 10 : -10) + 360) % 360)}
      >
        <LinearGradient colors={STOPS as [string, string, ...string[]]} start={{ x: 0, y: 0 }} end={{ x: 1, y: 0 }} style={styles.track} />
        <Animated.View style={[styles.knob, shadow.knob, { backgroundColor: color, borderColor: c.surface }, knob]} />
      </View>
    </GestureDetector>
  )
}

const styles = StyleSheet.create({
  hit: { height: 44, justifyContent: 'center' },
  track: { height: 14, borderRadius: 7, marginHorizontal: KNOB / 2 - 4 },
  knob: { position: 'absolute', left: 0, top: 8, width: KNOB, height: KNOB, borderRadius: KNOB / 2, borderWidth: 3 }
})

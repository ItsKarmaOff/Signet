import { useEffect, useState } from 'react'
import { StyleSheet, View, type LayoutChangeEvent } from 'react-native'
import Animated, { useAnimatedStyle, useSharedValue } from 'react-native-reanimated'
import { haptic } from '@/lib/haptics'
import type { LucideIcon } from './icons'
import { PressableScale, springs, useReduceMotion, useSpringTo } from './motion'
import { useColors } from './theme'
import { Text } from './Text'
import { radius, shadow } from './tokens'

export interface SegmentOption<T extends string> {
  value: T
  label: string
  icon?: LucideIcon
}

export interface SegmentedControlProps<T extends string> {
  options: readonly SegmentOption<T>[]
  value: T
  onChange: (value: T) => void
  testID?: string
}

/** Segmented selector (light / dark / auto…): the thumb springs from one segment to the next. */
export function SegmentedControl<T extends string>({ options, value, onChange, testID }: SegmentedControlProps<T>) {
  const c = useColors()
  const reduce = useReduceMotion()
  const spring = useSpringTo(reduce)
  const [width, setWidth] = useState(0)
  const x = useSharedValue(0)
  const index = Math.max(
    0,
    options.findIndex((o) => o.value === value)
  )
  const segment = width / options.length
  const knob = useAnimatedStyle(() => ({ transform: [{ translateX: x.value }] }))
  const onLayout = (e: LayoutChangeEvent) => {
    const w = e.nativeEvent.layout.width
    setWidth(w)
    x.value = (w / options.length) * index
  }
  // Follows the chosen value (also when it changes from outside).
  useEffect(() => {
    if (segment > 0) x.value = spring(segment * index, springs.snappy)
    // eslint-disable-next-line react-hooks/exhaustive-deps -- `spring` and `x` are stable for a given `reduce` value
  }, [segment, index, reduce])

  return (
    <View style={[styles.track, { backgroundColor: c.surface2 }]} onLayout={onLayout} accessibilityRole="tablist" testID={testID}>
      {segment > 0 ? <Animated.View style={[styles.knob, shadow.knob, { width: segment - 4, backgroundColor: c.surface }, knob]} /> : null}
      {options.map((o) => {
        const selected = o.value === value
        const Icon = o.icon
        return (
          <PressableScale
            key={o.value}
            accessibilityRole="tab"
            accessibilityLabel={o.label}
            accessibilityState={{ selected }}
            scaleTo={0.97}
            onPress={() => {
              if (!selected) {
                haptic('selection')
                onChange(o.value)
              }
            }}
            style={styles.item}
          >
            {Icon ? <Icon size={15} color={selected ? c.text : c.textSecondary} strokeWidth={2} /> : null}
            <Text variant="footnote" weight={selected ? '600' : '500'} color={selected ? 'text' : 'secondary'} numberOfLines={1}>
              {o.label}
            </Text>
          </PressableScale>
        )
      })}
    </View>
  )
}

const styles = StyleSheet.create({
  track: { height: 38, borderRadius: radius.control - 3, padding: 2, flexDirection: 'row' },
  knob: { position: 'absolute', top: 2, bottom: 2, left: 2, borderRadius: radius.control - 5 },
  item: { flex: 1, flexDirection: 'row', alignItems: 'center', justifyContent: 'center', gap: 5 }
})

// Action sheet: choices at the bottom of the screen (clear history, confirm a deletion), same look on iOS and Android.
import { useEffect, useState } from 'react'
import { Modal, Pressable, StyleSheet, View } from 'react-native'
import Animated, { useAnimatedStyle, useSharedValue, withTiming } from 'react-native-reanimated'
import { useSafeAreaInsets } from 'react-native-safe-area-context'
import { Check } from './icons'
import { PressableScale, springs, useReduceMotion, useSpringTo } from './motion'
import { useColors } from './theme'
import { Text } from './Text'
import { gutter, radius, space } from './tokens'

export interface ActionSheetAction {
  label: string
  onPress: () => void
  destructive?: boolean
  /** Checkmark on the right (current choice). */
  selected?: boolean
}

export interface ActionSheetProps {
  visible: boolean
  onClose: () => void
  title?: string
  message?: string
  actions: ActionSheetAction[]
  cancelLabel: string
}

export function ActionSheet({ visible, onClose, title, message, actions, cancelLabel }: ActionSheetProps) {
  const c = useColors()
  const insets = useSafeAreaInsets()
  const reduce = useReduceMotion()
  const spring = useSpringTo(reduce)
  const [mounted, setMounted] = useState(visible)
  const progress = useSharedValue(0)

  useEffect(() => {
    if (visible) {
      setMounted(true)
      progress.value = spring(1, springs.default)
      return
    }
    progress.value = withTiming(0, { duration: reduce ? 0 : 180 })
    const timer = setTimeout(() => setMounted(false), reduce ? 0 : 200)
    return () => clearTimeout(timer)
    // eslint-disable-next-line react-hooks/exhaustive-deps -- only visibility restarts the animation
  }, [visible])

  const backdrop = useAnimatedStyle(() => ({ opacity: progress.value }))
  const panel = useAnimatedStyle(() => ({ opacity: Math.min(1, progress.value * 2), transform: [{ translateY: (1 - progress.value) * 320 }] }))

  if (!mounted) return null
  return (
    <Modal transparent visible animationType="none" onRequestClose={onClose} statusBarTranslucent>
      <View style={styles.fill}>
        <Animated.View style={[StyleSheet.absoluteFill, { backgroundColor: c.scrim }, backdrop]}>
          <Pressable style={styles.fill} onPress={onClose} accessibilityRole="button" accessibilityLabel={cancelLabel} />
        </Animated.View>
        <Animated.View style={[styles.panel, { paddingBottom: Math.max(insets.bottom, space.md) }, panel]} pointerEvents="box-none">
          <View style={[styles.group, { backgroundColor: c.surface }]}>
            {title || message ? (
              <View style={[styles.header, { borderBottomColor: c.separator }]}>
                {title ? (
                  <Text variant="footnote" weight="600" color="secondary" align="center">
                    {title}
                  </Text>
                ) : null}
                {message ? (
                  <Text variant="footnote" color="secondary" align="center">
                    {message}
                  </Text>
                ) : null}
              </View>
            ) : null}
            {actions.map((a, i) => (
              <View key={a.label}>
                {i > 0 ? <View style={[styles.separator, { backgroundColor: c.separator }]} /> : null}
                <PressableScale
                  accessibilityRole="button"
                  accessibilityState={{ selected: !!a.selected }}
                  haptic={a.destructive ? 'warning' : 'selection'}
                  scaleTo={0.99}
                  onPress={() => {
                    onClose()
                    a.onPress()
                  }}
                  style={styles.action}
                >
                  <Text variant="body" color={a.destructive ? 'danger' : 'accent'} numberOfLines={1}>
                    {a.label}
                  </Text>
                  {a.selected ? <Check size={18} color={c.accent} strokeWidth={2.4} style={styles.check} /> : null}
                </PressableScale>
              </View>
            ))}
          </View>
          <PressableScale accessibilityRole="button" onPress={onClose} scaleTo={0.99} style={[styles.group, styles.action, { backgroundColor: c.surface }]}>
            <Text variant="body" weight="600" color="accent">
              {cancelLabel}
            </Text>
          </PressableScale>
        </Animated.View>
      </View>
    </Modal>
  )
}

const styles = StyleSheet.create({
  fill: { flex: 1 },
  panel: { position: 'absolute', left: 0, right: 0, bottom: 0, paddingHorizontal: gutter - 4, gap: space.sm },
  group: { borderRadius: radius.card, overflow: 'hidden' },
  header: { paddingVertical: space.md, paddingHorizontal: space.lg, gap: space.xxs, borderBottomWidth: StyleSheet.hairlineWidth },
  separator: { height: StyleSheet.hairlineWidth },
  action: { minHeight: 56, alignItems: 'center', justifyContent: 'center', paddingHorizontal: space.xxxl },
  check: { position: 'absolute', right: space.lg }
})

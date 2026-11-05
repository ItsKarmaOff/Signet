// Native fill bar above the keyboard: compact chips for saved addresses or cards, and "Manage".
// It does not take focus from the web field: chips are RN views, the keyboard stays up (the WKWebView keeps first responder).
import { ScrollView, StyleSheet } from 'react-native'
import Animated, { FadeInDown, FadeOutDown, useAnimatedKeyboard, useAnimatedStyle } from 'react-native-reanimated'
import { useT } from '@/lib/i18n'
import { GlassBar, PressableScale, Text, gutter, icons, radius, space, useColors, useReduceMotion } from '@/ui'

export interface BarItem {
  id: string
  label: string
  /** Accessibility label. */
  a11y: string
}

interface Props {
  kind: 'address' | 'card'
  /** Nothing to show: the bar is hidden (the component stays mounted to follow the keyboard). */
  items: BarItem[]
  /** Height at the bottom of the screen that the tab's view does not cover (the keyboard offset is relative to the screen). */
  bottomInset: number
  onPick: (id: string) => void
  onManage: () => void
}

export function AutofillBar({ kind, items, bottomInset, onPick, onManage }: Props) {
  const t = useT()
  const c = useColors()
  const reduce = useReduceMotion()
  const keyboard = useAnimatedKeyboard()
  const lift = useAnimatedStyle(() => ({ transform: [{ translateY: -Math.max(0, keyboard.height.value - bottomInset) }] }))
  const Icon = kind === 'card' ? icons.CreditCard : icons.MapPin
  return (
    <Animated.View style={[styles.host, lift]} pointerEvents="box-none">
      {items.length > 0 ? (
        <Animated.View entering={reduce ? undefined : FadeInDown.duration(140)} exiting={reduce ? undefined : FadeOutDown.duration(100)}>
          <GlassBar edge="top" style={styles.bar}>
            <ScrollView
              horizontal
              showsHorizontalScrollIndicator={false}
              keyboardShouldPersistTaps="always"
              accessibilityLabel={t('autofill.fillBarLabel')}
              contentContainerStyle={styles.row}
            >
              {items.map((item) => (
                <PressableScale
                  key={item.id}
                  accessibilityRole="button"
                  accessibilityLabel={item.a11y}
                  haptic="light"
                  onPress={() => onPick(item.id)}
                  style={[styles.chip, { backgroundColor: c.surface2 }]}
                >
                  <Icon size={15} color={c.accent} strokeWidth={2.2} />
                  <Text variant="subhead" numberOfLines={1} style={styles.label}>
                    {item.label}
                  </Text>
                </PressableScale>
              ))}
              <PressableScale
                accessibilityRole="button"
                accessibilityLabel={t('autofill.manage')}
                haptic="light"
                onPress={onManage}
                style={[styles.chip, { backgroundColor: c.surface2 }]}
              >
                <icons.Settings size={15} color={c.textSecondary} strokeWidth={2.2} />
                <Text variant="subhead" color="secondary">
                  {t('autofill.manage')}
                </Text>
              </PressableScale>
            </ScrollView>
          </GlassBar>
        </Animated.View>
      ) : null}
    </Animated.View>
  )
}

const styles = StyleSheet.create({
  host: { position: 'absolute', left: 0, right: 0, bottom: 0 },
  bar: { paddingVertical: space.sm },
  row: { alignItems: 'center', gap: space.sm, paddingHorizontal: gutter },
  chip: { flexDirection: 'row', alignItems: 'center', gap: space.xs, height: 36, paddingHorizontal: space.md, borderRadius: radius.pill },
  label: { maxWidth: 240 }
})

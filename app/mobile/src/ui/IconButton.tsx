import { StyleSheet, View } from 'react-native'
import { PressableScale } from './motion'
import { useColors } from './theme'
import { Text } from './Text'
import type { LucideIcon } from './icons'
import { hit, radius } from './tokens'

export interface IconButtonProps {
  icon: LucideIcon
  /** Accessibility label (required: the button has no text). */
  label: string
  onPress?: () => void
  onLongPress?: () => void
  disabled?: boolean
  /** Icon size (default 22); the touch target stays at least 44 pt. */
  size?: number
  tone?: 'default' | 'accent' | 'muted' | 'danger'
  /** Background badge (for a standalone button over content). */
  filled?: boolean
  /** Counter shown on the icon (number of tabs…). */
  badge?: string | number
  testID?: string
}

/** Icon button: 44 pt target, spring press, light haptic feedback. */
export function IconButton({ icon: Icon, label, onPress, onLongPress, disabled, size = 22, tone = 'default', filled, badge, testID }: IconButtonProps) {
  const c = useColors()
  const color = tone === 'accent' ? c.accent : tone === 'danger' ? c.danger : tone === 'muted' ? c.textSecondary : c.text
  return (
    <PressableScale
      accessibilityRole="button"
      accessibilityLabel={label}
      accessibilityState={{ disabled: !!disabled }}
      testID={testID}
      disabled={disabled}
      onPress={onPress}
      onLongPress={onLongPress}
      haptic="light"
      hitSlop={4}
      style={[styles.box, filled && { backgroundColor: c.surface2, borderRadius: radius.pill }, disabled && styles.disabled]}
    >
      <View>
        <Icon size={size} color={color} strokeWidth={1.9} />
        {badge !== undefined ? (
          <View style={[styles.badge, { backgroundColor: c.accent }]}>
            <Text variant="caption2" color="onAccent" weight="600" style={styles.badgeText}>
              {badge}
            </Text>
          </View>
        ) : null}
      </View>
    </PressableScale>
  )
}

const styles = StyleSheet.create({
  box: { width: hit, height: hit, alignItems: 'center', justifyContent: 'center' },
  disabled: { opacity: 0.3 },
  badge: {
    position: 'absolute',
    top: -6,
    right: -10,
    minWidth: 16,
    height: 16,
    borderRadius: 8,
    paddingHorizontal: 4,
    alignItems: 'center',
    justifyContent: 'center'
  },
  badgeText: { fontSize: 10, lineHeight: 12 }
})

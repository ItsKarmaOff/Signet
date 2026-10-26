import { ActivityIndicator, StyleSheet, View, type StyleProp, type ViewStyle } from 'react-native'
import { PressableScale } from './motion'
import { useColors } from './theme'
import { Text } from './Text'
import type { LucideIcon } from './icons'
import { radius } from './tokens'

export interface ButtonProps {
  title: string
  onPress?: () => void
  /** `primary`: accent background; `secondary`: neutral background; `plain`: text only; `destructive`: deleting action. */
  variant?: 'primary' | 'secondary' | 'plain' | 'destructive'
  size?: 'md' | 'lg'
  icon?: LucideIcon
  loading?: boolean
  disabled?: boolean
  /** Takes all available width. */
  fullWidth?: boolean
  style?: StyleProp<ViewStyle>
  testID?: string
}

export function Button({ title, onPress, variant = 'primary', size = 'md', icon: Icon, loading, disabled, fullWidth, style, testID }: ButtonProps) {
  const c = useColors()
  const bg = variant === 'primary' ? c.accent : variant === 'destructive' ? c.danger : variant === 'secondary' ? c.surface2 : 'transparent'
  const fg = variant === 'primary' ? c.onAccent : variant === 'destructive' ? c.onDanger : variant === 'secondary' ? c.text : c.accent
  return (
    <PressableScale
      accessibilityRole="button"
      accessibilityLabel={title}
      accessibilityState={{ disabled: !!disabled, busy: !!loading }}
      testID={testID}
      disabled={disabled || loading}
      onPress={onPress}
      haptic="light"
      style={[
        styles.base,
        size === 'lg' ? styles.lg : styles.md,
        { backgroundColor: bg },
        fullWidth && styles.full,
        (disabled || loading) && styles.disabled,
        style
      ]}
    >
      <View style={styles.row}>
        {loading ? <ActivityIndicator color={fg} /> : Icon ? <Icon size={size === 'lg' ? 20 : 18} color={fg} strokeWidth={2} /> : null}
        <Text variant={size === 'lg' ? 'headline' : 'callout'} weight="600" color={fg}>
          {title}
        </Text>
      </View>
    </PressableScale>
  )
}

const styles = StyleSheet.create({
  base: { borderRadius: radius.control, alignItems: 'center', justifyContent: 'center', alignSelf: 'flex-start' },
  md: { minHeight: 44, paddingHorizontal: 18 },
  lg: { minHeight: 52, paddingHorizontal: 24 },
  full: { alignSelf: 'stretch' },
  disabled: { opacity: 0.45 },
  row: { flexDirection: 'row', alignItems: 'center', gap: 8 }
})

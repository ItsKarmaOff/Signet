import type { ReactNode } from 'react'
import { Platform, StyleSheet, View, type StyleProp, type ViewStyle } from 'react-native'
import { BlurView } from 'expo-blur'
import { withAlpha } from '@signet/shared/mobile/color'
import { useReduceTransparency } from './motion'
import { useTheme } from './theme'
import { radius, shadow } from './tokens'

export interface GlassBarProps {
  children?: ReactNode
  /** Which edge carries the hairline: `top` for a bar at the bottom of the screen, `bottom` for a bar at the top. */
  edge?: 'top' | 'bottom' | 'none'
  /** Floating bar: rounded corners and shadow (for the address bar), otherwise full width. */
  floating?: boolean
  /** Blur strength (0-100). */
  intensity?: number
  style?: StyleProp<ViewStyle>
}

/** Glass bar: blur of the content passing underneath, veil tinted by the space's hue. Opaque if "Reduce transparency". */
export function GlassBar({ children, edge = 'none', floating, intensity = 70, style }: GlassBarProps) {
  const { colors, scheme } = useTheme()
  const opaque = useReduceTransparency()
  // Android: no reliable native blur in Expo Go, fall back to a nearly opaque veil.
  const blur = !opaque && Platform.OS === 'ios'
  const veil = blur ? withAlpha(colors.tint, scheme === 'dark' ? 0.1 : 0.07) : withAlpha(colors.surface, opaque ? 1 : 0.95)
  return (
    <View
      style={[
        floating && styles.floating,
        floating && shadow.pop,
        edge === 'top' && { borderTopWidth: StyleSheet.hairlineWidth, borderColor: colors.separator },
        edge === 'bottom' && { borderBottomWidth: StyleSheet.hairlineWidth, borderColor: colors.separator },
        floating && { borderWidth: StyleSheet.hairlineWidth, borderColor: colors.separator },
        style
      ]}
    >
      <View style={[StyleSheet.absoluteFill, floating && styles.clip]} pointerEvents="none">
        {blur ? (
          <BlurView intensity={intensity} tint={scheme === 'dark' ? 'systemChromeMaterialDark' : 'systemChromeMaterialLight'} style={StyleSheet.absoluteFill} />
        ) : null}
        <View style={[StyleSheet.absoluteFill, { backgroundColor: blur ? colors.glassTint : undefined }, { backgroundColor: veil }]} />
      </View>
      {children}
    </View>
  )
}

const styles = StyleSheet.create({
  floating: { borderRadius: radius.group + 6 },
  clip: { borderRadius: radius.group + 6, overflow: 'hidden' }
})

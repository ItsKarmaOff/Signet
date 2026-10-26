import { StyleSheet, Text as RNText, View } from 'react-native'
import { LinearGradient } from 'expo-linear-gradient'
import { previewColors } from '@signet/shared/mobile/color'
import type { SpaceTheme } from '@signet/shared/types'

export interface SpaceBadgeProps {
  icon: string
  hue: number
  theme?: SpaceTheme
  size?: number
}

/** Space badge: its emoji on its color (or gradient), like the desktop badge. */
export function SpaceBadge({ icon, hue, theme, size = 32 }: SpaceBadgeProps) {
  const colors = previewColors(theme, hue)
  const fill = colors.length > 1 ? colors : [colors[0], colors[0]]
  return (
    <View
      style={{ width: size, height: size, borderRadius: size / 2, overflow: 'hidden' }}
      accessibilityElementsHidden
      importantForAccessibility="no-hide-descendants"
    >
      <LinearGradient colors={fill as [string, string, ...string[]]} start={{ x: 0, y: 0 }} end={{ x: 1, y: 1 }} style={styles.center}>
        <View style={styles.center}>
          <EmojiText icon={icon} size={size} />
        </View>
      </LinearGradient>
    </View>
  )
}

function EmojiText({ icon, size }: { icon: string; size: number }) {
  return <RNText style={{ fontSize: size * 0.52, lineHeight: size * 0.64, textAlign: 'center' }}>{icon}</RNText>
}

const styles = StyleSheet.create({ center: { flex: 1, alignItems: 'center', justifyContent: 'center' } })

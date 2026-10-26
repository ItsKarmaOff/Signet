// Live preview of the chosen theme: a mini page in light or dark colors, which changes as soon as the selector is touched.
import { StyleSheet, useColorScheme, View } from 'react-native'
import { LinearGradient } from 'expo-linear-gradient'
import { withAlpha, type Scheme } from '@signet/shared/mobile/color'
import type { Theme } from '@signet/shared/types'
import { useT } from '@/lib/i18n'
import { Text, buildTheme, icons, radius, space, useTheme } from '@/ui'

export function ThemePreview({ choice }: { choice: Theme }) {
  const t = useT()
  const { hue, gradient, isPrivate } = useTheme()
  const system = useColorScheme() === 'dark' ? 'dark' : 'light'
  const scheme: Scheme = isPrivate ? 'dark' : choice === 'system' ? system : choice
  const p = buildTheme(hue, scheme).colors
  return (
    <View
      style={[styles.frame, { backgroundColor: p.background, borderColor: p.separator }]}
      accessible
      accessibilityLabel={t('settings.themePreview')}
      accessibilityRole="image"
    >
      <LinearGradient
        colors={gradient.colors.map((x) => withAlpha(x, scheme === 'dark' ? 0.28 : 0.35)) as [string, string, ...string[]]}
        start={gradient.start}
        end={gradient.end}
        style={StyleSheet.absoluteFill}
      />
      <View style={styles.page}>
        <View style={[styles.bar, { backgroundColor: p.accent }]} />
        <View style={[styles.line, { backgroundColor: p.textTertiary, width: '86%' }]} />
        <View style={[styles.line, { backgroundColor: p.textTertiary, width: '64%' }]} />
        <View style={[styles.card, { backgroundColor: p.surface }]}>
          <View style={[styles.dot, { backgroundColor: p.accent }]} />
          <View style={[styles.line, { backgroundColor: p.textSecondary, width: '50%', marginVertical: 0 }]} />
        </View>
      </View>
      <View style={[styles.address, { backgroundColor: p.surface2 }]}>
        <icons.Lock size={12} color={p.textSecondary} strokeWidth={2.2} />
        <Text variant="caption" style={{ color: p.textSecondary }}>
          {t('settings.previewSite')}
        </Text>
      </View>
    </View>
  )
}

const styles = StyleSheet.create({
  frame: { height: 150, borderRadius: radius.group, borderWidth: StyleSheet.hairlineWidth, overflow: 'hidden', justifyContent: 'space-between' },
  page: { padding: space.lg, gap: space.sm },
  bar: { width: 54, height: 8, borderRadius: 4, marginBottom: space.xs },
  line: { height: 6, borderRadius: 3, opacity: 0.5 },
  card: { flexDirection: 'row', alignItems: 'center', gap: space.sm, padding: space.sm, borderRadius: radius.control, marginTop: space.xs, width: '70%' },
  dot: { width: 14, height: 14, borderRadius: 7 },
  address: {
    alignSelf: 'center',
    flexDirection: 'row',
    alignItems: 'center',
    gap: space.xs,
    paddingVertical: space.xs,
    paddingHorizontal: space.md,
    borderRadius: radius.pill,
    marginBottom: space.md
  }
})

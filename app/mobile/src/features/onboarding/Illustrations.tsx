// Welcome page illustrations: simple shapes in the theme colors, gently floating (still when motion is reduced).
import { useEffect, type ReactNode } from 'react'
import { StyleSheet, View } from 'react-native'
import Animated, { Easing, useAnimatedStyle, useSharedValue, withDelay, withRepeat, withTiming } from 'react-native-reanimated'
import { LinearGradient } from 'expo-linear-gradient'
import { accentColor, tintColor, withAlpha } from '@signet/shared/mobile/color'
import { icons, radius, shadow, space, useReduceMotion, useTheme, type LucideIcon } from '@/ui'

/** Floats a few points, in a loop, with an offset specific to each element. */
function Floating({ children, delay = 0, amplitude = 8, style }: { children: ReactNode; delay?: number; amplitude?: number; style?: object }) {
  const reduce = useReduceMotion()
  const phase = useSharedValue(0)
  useEffect(() => {
    if (reduce) {
      phase.value = 0.5
      return
    }
    phase.value = withDelay(delay, withRepeat(withTiming(1, { duration: 2800, easing: Easing.inOut(Easing.quad) }), -1, true))
    // eslint-disable-next-line react-hooks/exhaustive-deps -- only the motion setting restarts the loop
  }, [reduce])
  const animated = useAnimatedStyle(() => ({ transform: [{ translateY: (phase.value - 0.5) * 2 * amplitude }] }))
  return <Animated.View style={[style, animated]}>{children}</Animated.View>
}

function Chip({ icon: Icon, delay, style }: { icon: LucideIcon; delay: number; style: object }) {
  const { colors } = useTheme()
  return (
    <Floating delay={delay} amplitude={6} style={[styles.chip, { backgroundColor: colors.surface }, shadow.card, style]}>
      <Icon size={20} color={colors.accent} strokeWidth={1.9} />
    </Floating>
  )
}

/** Page 1: the browser. */
export function WelcomeArt() {
  const { gradient } = useTheme()
  return (
    <View style={styles.stage}>
      <Floating amplitude={5}>
        <LinearGradient colors={gradient.colors as [string, string, ...string[]]} start={gradient.start} end={gradient.end} style={[styles.hero, shadow.pop]}>
          <icons.Globe size={64} color="#ffffff" strokeWidth={1.5} />
        </LinearGradient>
      </Floating>
      <Chip icon={icons.Search} delay={300} style={{ top: 10, left: 14 }} />
      <Chip icon={icons.Bookmark} delay={900} style={{ top: 34, right: 6 }} />
      <Chip icon={icons.Shield} delay={1500} style={{ bottom: 16, left: 44 }} />
    </View>
  )
}

/** Page 2 : trois espaces, trois teintes. */
export function SpacesArt() {
  const { scheme, colors } = useTheme()
  const hues = [215, 150, 20]
  return (
    <View style={styles.stage}>
      {hues.map((hue, i) => (
        <Floating key={hue} delay={i * 450} amplitude={6} style={[styles.spaceCard, { top: 6 + i * 44, left: 24 + i * 22, zIndex: i }]}>
          <View style={[styles.spaceInner, { backgroundColor: tintColor(hue, scheme), borderColor: withAlpha(accentColor(hue, scheme), 0.5) }, shadow.card]}>
            <View style={[styles.spaceDot, { backgroundColor: accentColor(hue, scheme) }]} />
            <View style={styles.spaceLines}>
              <View style={[styles.line, { backgroundColor: accentColor(hue, scheme), width: 70 }]} />
              <View style={[styles.line, { backgroundColor: colors.textTertiary, width: 46 }]} />
            </View>
          </View>
        </Floating>
      ))}
    </View>
  )
}

/** Page 3: private browsing and blocking. */
export function PrivacyArt() {
  const { colors } = useTheme()
  return (
    <View style={styles.stage}>
      <Floating amplitude={5}>
        <View style={[styles.hero, styles.privateHero, shadow.pop]}>
          <icons.VenetianMask size={64} color="#e9e4f2" strokeWidth={1.5} />
        </View>
      </Floating>
      <Chip icon={icons.Shield} delay={200} style={{ top: 18, right: 20 }} />
      <Chip icon={icons.Lock} delay={1000} style={{ bottom: 20, left: 24 }} />
      <Floating delay={600} amplitude={5} style={[styles.chip, { backgroundColor: colors.surface, bottom: 40, right: 12 }, shadow.card]}>
        <icons.EyeOff size={20} color={colors.accent} strokeWidth={1.9} />
      </Floating>
    </View>
  )
}

/** Page 4 : personnalisation. */
export function PersonalizeArt() {
  return (
    <View style={[styles.stage, styles.small]}>
      <Floating amplitude={4}>
        <Chip icon={icons.Palette} delay={0} style={{ position: 'relative' }} />
      </Floating>
    </View>
  )
}

const styles = StyleSheet.create({
  stage: { width: 260, height: 200, alignSelf: 'center' },
  small: { height: 56, justifyContent: 'center', alignItems: 'center' },
  hero: { width: 128, height: 128, borderRadius: 34, alignItems: 'center', justifyContent: 'center', alignSelf: 'center', marginTop: 28 },
  privateHero: { backgroundColor: '#3b3446' },
  chip: { position: 'absolute', width: 44, height: 44, borderRadius: radius.control, alignItems: 'center', justifyContent: 'center' },
  spaceCard: { position: 'absolute' },
  spaceInner: {
    width: 150,
    height: 66,
    borderRadius: radius.group,
    borderWidth: 1,
    flexDirection: 'row',
    alignItems: 'center',
    gap: space.md,
    paddingHorizontal: space.lg
  },
  spaceDot: { width: 28, height: 28, borderRadius: 14 },
  spaceLines: { gap: space.sm },
  line: { height: 6, borderRadius: 3, opacity: 0.8 }
})

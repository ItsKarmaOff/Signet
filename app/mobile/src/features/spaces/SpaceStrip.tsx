import { useEffect, useRef, type ComponentRef } from 'react'
import { ScrollView, StyleSheet, View } from 'react-native'
import Animated, { LinearTransition } from 'react-native-reanimated'
import { withAlpha } from '@signet/shared/mobile/color'
import type { MobileSpace } from '@signet/shared/mobile/workspace'
import { haptic } from '@/lib/haptics'
import { useT } from '@/lib/i18n'
import { PressableScale, Text, gutter, icons, radius, space, useColors, useReduceMotion } from '@/ui'
import { SpaceBadge } from './SpaceBadge'

export interface SpaceStripProps {
  spaces: readonly MobileSpace[]
  activeId: string
  /** No visually active space (private browsing). */
  dimmed?: boolean
  onSelect: (id: string) => void
  onEdit: (id: string) => void
  /** Absent: no "+" button (space limit reached). */
  onAdd?: () => void
}

/** Row of spaces: emoji badge, the name for the selected space. Tap changes space, long-press opens the editor. */
export function SpaceStrip({ spaces, activeId, dimmed, onSelect, onEdit, onAdd }: SpaceStripProps) {
  const t = useT()
  const c = useColors()
  const reduce = useReduceMotion()
  const scroll = useRef<ComponentRef<typeof ScrollView>>(null)
  const xs = useRef<Record<string, { x: number; w: number }>>({})
  const width = useRef(0)

  // Keeps the selected space at the center of the row.
  useEffect(() => {
    const at = xs.current[activeId]
    if (at && width.current) scroll.current?.scrollTo({ x: Math.max(0, at.x + at.w / 2 - width.current / 2), animated: true })
  }, [activeId])

  return (
    <ScrollView
      ref={scroll}
      horizontal
      showsHorizontalScrollIndicator={false}
      onLayout={(e) => (width.current = e.nativeEvent.layout.width)}
      contentContainerStyle={styles.content}
      style={styles.scroll}
    >
      {spaces.map((s) => {
        const selected = s.id === activeId && !dimmed
        return (
          <Animated.View
            key={s.id}
            layout={reduce ? undefined : LinearTransition.springify().damping(24).stiffness(260)}
            onLayout={(e) => (xs.current[s.id] = { x: e.nativeEvent.layout.x, w: e.nativeEvent.layout.width })}
          >
            <PressableScale
              accessibilityRole="button"
              accessibilityLabel={t('tabs.switchSpace', { name: s.name })}
              accessibilityHint={t('spaces.editHint')}
              accessibilityState={{ selected }}
              onPress={() => {
                if (s.id !== activeId || dimmed) {
                  haptic('selection')
                  onSelect(s.id)
                }
              }}
              onLongPress={() => {
                haptic('medium')
                onEdit(s.id)
              }}
              delayLongPress={350}
              style={[styles.chip, { backgroundColor: selected ? withAlpha(c.tint, 0.2) : 'transparent' }]}
            >
              <SpaceBadge icon={s.icon} hue={s.hue} theme={s.theme} size={30} />
              {selected ? (
                <Text variant="subhead" weight="600" numberOfLines={1} style={styles.name}>
                  {s.name}
                </Text>
              ) : null}
            </PressableScale>
          </Animated.View>
        )
      })}
      {onAdd ? (
        <View>
          <PressableScale
            accessibilityRole="button"
            accessibilityLabel={t('spaces.new')}
            onPress={() => {
              haptic('light')
              onAdd()
            }}
            style={[styles.chip, styles.add, { backgroundColor: c.surface2 }]}
          >
            <icons.Plus size={16} color={c.textSecondary} strokeWidth={2.2} />
          </PressableScale>
        </View>
      ) : null}
    </ScrollView>
  )
}

const styles = StyleSheet.create({
  scroll: { flexGrow: 0 },
  content: { paddingHorizontal: gutter, gap: space.xs, alignItems: 'center' },
  chip: { flexDirection: 'row', alignItems: 'center', height: 38, borderRadius: radius.pill, paddingHorizontal: 4, gap: space.sm },
  name: { maxWidth: 140, paddingRight: space.md },
  add: { width: 38, justifyContent: 'center' }
})

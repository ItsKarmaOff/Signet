// Find in page: bar above the keyboard with field, counter, previous / next and close.
import { useEffect, useRef, useState, type ComponentRef } from 'react'
import { StyleSheet, TextInput, View } from 'react-native'
import Animated, { useAnimatedKeyboard, useAnimatedStyle } from 'react-native-reanimated'
import type { FindResult } from '@signet/shared/mobile/find'
import { useT } from '@/lib/i18n'
import { Button, GlassBar, IconButton, Text, fontStyle, icons, radius, space, useColors, useTheme } from '@/ui'

export const FIND_BAR_HEIGHT = 56

interface Props {
  result: FindResult | null
  safeBottom: number
  onQuery: (query: string) => void
  onStep: (direction: 1 | -1) => void
  onClose: () => void
}

export function FindBar({ result, safeBottom, onQuery, onStep, onClose }: Props) {
  const t = useT()
  const c = useColors()
  const { scheme } = useTheme()
  const input = useRef<ComponentRef<typeof TextInput>>(null)
  const [text, setText] = useState('')
  const keyboard = useAnimatedKeyboard()
  const lift = useAnimatedStyle(() => ({ transform: [{ translateY: -keyboard.height.value }] }))
  const pad = useAnimatedStyle(() => ({ paddingBottom: Math.max(space.sm, safeBottom - keyboard.height.value) }))

  // Debounced search: not one script per keystroke.
  useEffect(() => {
    const timer = setTimeout(() => onQuery(text), 180)
    return () => clearTimeout(timer)
    // eslint-disable-next-line react-hooks/exhaustive-deps -- `onQuery` changes on every render of the parent
  }, [text])

  const count = result?.count ?? 0
  const label = !text ? '' : count === 0 ? t('find.none') : t('find.count', { index: (result?.index ?? 0) + 1, total: count })

  return (
    <Animated.View style={[styles.host, lift]}>
      <GlassBar edge="top" style={styles.bar}>
        <Animated.View style={[styles.row, pad]}>
          <View style={[styles.field, { backgroundColor: c.surface2 }]}>
            <icons.Search size={16} color={c.textTertiary} strokeWidth={2} />
            <TextInput
              ref={input}
              value={text}
              onChangeText={setText}
              autoFocus
              autoCapitalize="none"
              autoCorrect={false}
              returnKeyType="search"
              clearButtonMode="while-editing"
              onSubmitEditing={() => onStep(1)}
              placeholder={t('find.placeholder')}
              placeholderTextColor={c.textTertiary}
              keyboardAppearance={scheme}
              selectionColor={c.accent}
              accessibilityLabel={t('find.placeholder')}
              style={[styles.input, fontStyle('callout'), { color: c.text }]}
            />
            {label ? (
              <Text variant="caption" color="secondary" numberOfLines={1}>
                {label}
              </Text>
            ) : null}
          </View>
          <IconButton icon={icons.ChevronUp} label={t('find.previous')} onPress={() => onStep(-1)} disabled={count === 0} size={20} />
          <IconButton icon={icons.ChevronDown} label={t('find.next')} onPress={() => onStep(1)} disabled={count === 0} size={20} />
          <Button title={t('find.done')} variant="plain" onPress={onClose} />
        </Animated.View>
      </GlassBar>
    </Animated.View>
  )
}

const styles = StyleSheet.create({
  host: { position: 'absolute', left: 0, right: 0, bottom: 0 },
  bar: { paddingTop: space.sm, paddingHorizontal: space.sm },
  row: { flexDirection: 'row', alignItems: 'center', gap: 0 },
  field: {
    flex: 1,
    height: 40,
    flexDirection: 'row',
    alignItems: 'center',
    gap: space.sm,
    paddingHorizontal: space.md,
    borderRadius: radius.control,
    marginRight: space.xs
  },
  input: { flex: 1, height: 40 }
})

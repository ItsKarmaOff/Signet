import type { ReactNode } from 'react'
import { ScrollView, StyleSheet, View, type StyleProp, type ViewStyle } from 'react-native'
import { SafeAreaView, type Edge } from 'react-native-safe-area-context'
import { Button } from './Button'
import { useColors } from './theme'
import { Text } from './Text'
import { gutter, space } from './tokens'

export interface ScreenProps {
  children?: ReactNode
  /** Scrolling content (ScrollView); otherwise a plain view. */
  scroll?: boolean
  /** Edges protected by safe areas. Default: bottom only (the stack's native header handles the top). Without a native header: `['top', 'bottom']`. */
  edges?: Edge[]
  /** Side and vertical padding around the content (default: no). */
  padded?: boolean
  style?: StyleProp<ViewStyle>
  contentStyle?: StyleProp<ViewStyle>
  testID?: string
}

/** Base screen: theme background, safe areas, optional scrolling. All screens in `app/` start with it. */
export function Screen({ children, scroll, edges = ['bottom'], padded, style, contentStyle, testID }: ScreenProps) {
  const c = useColors()
  const pad = padded && { paddingHorizontal: gutter, paddingVertical: space.lg }
  return (
    <SafeAreaView edges={edges} style={[styles.fill, { backgroundColor: c.background }, style]} testID={testID}>
      {scroll ? (
        <ScrollView
          style={styles.fill}
          contentContainerStyle={[pad, styles.scrollContent, contentStyle]}
          contentInsetAdjustmentBehavior="automatic"
          keyboardShouldPersistTaps="handled"
          keyboardDismissMode="on-drag"
        >
          {children}
        </ScrollView>
      ) : (
        <View style={[styles.fill, pad, contentStyle]}>{children}</View>
      )}
    </SafeAreaView>
  )
}

export interface SheetProps {
  title: string
  /** Right button text ("OK"). */
  doneLabel: string
  onDone: () => void
  children?: ReactNode
  scroll?: boolean
}

/**
 * Sheet content (screen presented as `formSheet`): native handle above, centered title, "OK" button on the right.
 * Sheets are declared in `_layout.tsx` with `presentation: 'formSheet'` (see app/spaces/edit/[id].tsx).
 */
export function Sheet({ title, doneLabel, onDone, children, scroll = true }: SheetProps) {
  const c = useColors()
  return (
    <View style={[styles.fill, { backgroundColor: c.background }]}>
      <View style={styles.sheetHeader}>
        <View style={styles.sheetSide} />
        <Text variant="headline" align="center" numberOfLines={1} style={styles.sheetTitle} accessibilityRole="header">
          {title}
        </Text>
        <View style={[styles.sheetSide, styles.sheetRight]}>
          <Button title={doneLabel} variant="plain" onPress={onDone} />
        </View>
      </View>
      <Screen scroll={scroll}>{children}</Screen>
    </View>
  )
}

const styles = StyleSheet.create({
  fill: { flex: 1 },
  scrollContent: { paddingTop: space.lg, paddingBottom: space.xxxl },
  sheetHeader: { flexDirection: 'row', alignItems: 'center', paddingTop: space.lg, paddingHorizontal: space.sm },
  sheetSide: { width: 90 },
  sheetRight: { alignItems: 'flex-end' },
  sheetTitle: { flex: 1 }
})

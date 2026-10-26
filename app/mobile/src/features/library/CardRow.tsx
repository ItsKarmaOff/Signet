import type { ReactNode } from 'react'
import { StyleSheet, View } from 'react-native'
import { gutter, radius, useColors } from '@/ui'

export interface CardRowProps {
  children: ReactNode
  first: boolean
  last: boolean
  /** Separator line inset (aligned with the text). */
  inset?: number
}

/** A row of a card list: surface background, rounded corners at the top of the first and the bottom of the last, thin line between rows. */
export function CardRow({ children, first, last, inset = 56 }: CardRowProps) {
  const c = useColors()
  return (
    <View
      style={[
        styles.row,
        { backgroundColor: c.surface },
        first && { borderTopLeftRadius: radius.group, borderTopRightRadius: radius.group },
        last && { borderBottomLeftRadius: radius.group, borderBottomRightRadius: radius.group }
      ]}
    >
      {children}
      {last ? null : <View pointerEvents="none" style={[styles.separator, { backgroundColor: c.separator, left: inset }]} />}
    </View>
  )
}

const styles = StyleSheet.create({
  row: { marginHorizontal: gutter, overflow: 'hidden' },
  separator: { position: 'absolute', right: 0, bottom: 0, height: StyleSheet.hairlineWidth }
})

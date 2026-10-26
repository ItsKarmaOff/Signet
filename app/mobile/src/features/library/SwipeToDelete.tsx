import { useRef, type ReactNode } from 'react'
import { Pressable, StyleSheet } from 'react-native'
import Swipeable, { type SwipeableMethods } from 'react-native-gesture-handler/ReanimatedSwipeable'
import { haptic } from '@/lib/haptics'
import { Text, icons, space, useColors } from '@/ui'

const ACTION_WIDTH = 88

export interface SwipeToDeleteProps {
  children: ReactNode
  label: string
  onDelete: () => void
  enabled?: boolean
}

/** Swiping left reveals "Delete" (tap to confirm), like in Mail and Safari. */
export function SwipeToDelete({ children, label, onDelete, enabled = true }: SwipeToDeleteProps) {
  const c = useColors()
  const ref = useRef<SwipeableMethods>(null)
  return (
    <Swipeable
      ref={ref}
      enabled={enabled}
      friction={2}
      rightThreshold={36}
      overshootRight={false}
      onSwipeableOpenStartDrag={() => haptic('selection')}
      renderRightActions={() => (
        <Pressable
          accessibilityRole="button"
          accessibilityLabel={label}
          onPress={() => {
            haptic('medium')
            ref.current?.close()
            onDelete()
          }}
          style={[styles.action, { backgroundColor: c.danger }]}
        >
          <icons.Trash size={20} color={c.onDanger} strokeWidth={2} />
          <Text variant="caption" weight="600" style={{ color: c.onDanger }}>
            {label}
          </Text>
        </Pressable>
      )}
    >
      {children}
    </Swipeable>
  )
}

const styles = StyleSheet.create({
  action: { width: ACTION_WIDTH, alignItems: 'center', justifyContent: 'center', gap: space.xs }
})

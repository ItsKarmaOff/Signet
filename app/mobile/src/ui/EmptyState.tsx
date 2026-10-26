import { StyleSheet, View } from 'react-native'
import { Button } from './Button'
import type { LucideIcon } from './icons'
import { useColors } from './theme'
import { Text } from './Text'
import { space } from './tokens'

export interface EmptyStateProps {
  icon?: LucideIcon
  title: string
  message?: string
  action?: { label: string; onPress: () => void }
}

/** Empty screen or list: discreet icon, title, explanation and, if needed, an action. */
export function EmptyState({ icon: Icon, title, message, action }: EmptyStateProps) {
  const c = useColors()
  return (
    <View style={styles.box} accessibilityRole="summary">
      {Icon ? <Icon size={40} color={c.textTertiary} strokeWidth={1.4} /> : null}
      <Text variant="title3" align="center">
        {title}
      </Text>
      {message ? (
        <Text variant="subhead" color="secondary" align="center" style={styles.message}>
          {message}
        </Text>
      ) : null}
      {action ? <Button title={action.label} onPress={action.onPress} variant="secondary" style={styles.action} /> : null}
    </View>
  )
}

const styles = StyleSheet.create({
  box: { flex: 1, alignItems: 'center', justifyContent: 'center', gap: space.md, padding: space.xxxl },
  message: { maxWidth: 300 },
  action: { marginTop: space.sm, alignSelf: 'center' }
})

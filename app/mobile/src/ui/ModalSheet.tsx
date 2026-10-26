// Form sheet: iOS "page" window (handle, rounded background) or full screen on Android, with "Cancel" and a save button.
import type { ReactNode } from 'react'
import { KeyboardAvoidingView, Modal, ScrollView, StyleSheet, View } from 'react-native'
import { Button } from './Button'
import { useColors } from './theme'
import { Text } from './Text'
import { gutter, space } from './tokens'

export interface ModalSheetProps {
  visible: boolean
  title: string
  cancelLabel: string
  /** Right button text; without it, only "Cancel" (renamed by `cancelLabel`) closes the sheet. */
  saveLabel?: string
  saveDisabled?: boolean
  onSave?: () => void
  onClose: () => void
  children?: ReactNode
}

export function ModalSheet({ visible, title, cancelLabel, saveLabel, saveDisabled, onSave, onClose, children }: ModalSheetProps) {
  const c = useColors()
  return (
    <Modal visible={visible} animationType="slide" presentationStyle="pageSheet" onRequestClose={onClose}>
      {/* Android draws edge to edge: the window no longer shrinks under the keyboard, so we must offset ourselves. */}
      <KeyboardAvoidingView behavior="padding" style={[styles.fill, { backgroundColor: c.background }]}>
        <View style={styles.header}>
          <View style={[styles.side, styles.left]}>
            <Button title={cancelLabel} variant="plain" onPress={onClose} />
          </View>
          <Text variant="headline" align="center" numberOfLines={1} style={styles.title} accessibilityRole="header">
            {title}
          </Text>
          <View style={[styles.side, styles.right]}>
            {saveLabel ? <Button title={saveLabel} variant="plain" disabled={saveDisabled} onPress={onSave} /> : null}
          </View>
        </View>
        <ScrollView style={styles.fill} contentContainerStyle={styles.content} keyboardShouldPersistTaps="handled" keyboardDismissMode="interactive">
          {children}
        </ScrollView>
      </KeyboardAvoidingView>
    </Modal>
  )
}

const styles = StyleSheet.create({
  fill: { flex: 1 },
  header: { flexDirection: 'row', alignItems: 'center', paddingTop: space.md, paddingHorizontal: space.sm },
  side: { width: 96 },
  left: { alignItems: 'flex-start' },
  right: { alignItems: 'flex-end' },
  title: { flex: 1 },
  content: { paddingHorizontal: gutter, paddingTop: space.lg, paddingBottom: space.huge, gap: space.lg }
})

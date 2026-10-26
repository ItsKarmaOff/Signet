// Haptic feedback, turned off by the `haptics` setting. Never throws (simulator, devices without a haptic engine).
import * as Haptics from 'expo-haptics'
import { useStore } from '@/state/store'

export type HapticKind = 'light' | 'medium' | 'selection' | 'success' | 'warning' | 'error'

export function haptic(kind: HapticKind = 'light'): void {
  if (!useStore.getState().settings.haptics) return
  const run = async () => {
    if (kind === 'selection') return Haptics.selectionAsync()
    if (kind === 'success' || kind === 'warning' || kind === 'error') {
      return Haptics.notificationAsync(
        kind === 'success'
          ? Haptics.NotificationFeedbackType.Success
          : kind === 'warning'
            ? Haptics.NotificationFeedbackType.Warning
            : Haptics.NotificationFeedbackType.Error
      )
    }
    return Haptics.impactAsync(kind === 'medium' ? Haptics.ImpactFeedbackStyle.Medium : Haptics.ImpactFeedbackStyle.Light)
  }
  run().catch(() => {})
}

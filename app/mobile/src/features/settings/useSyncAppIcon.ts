import { useEffect } from 'react'
import { useStore } from '@/state'
import { currentAppIcon } from './appIcon'

/** At launch: if the OS shows a different icon than the setting (changed from outside, reinstall), the setting follows the OS. */
export function useSyncAppIcon(): void {
  const setSettings = useStore((s) => s.setSettings)
  useEffect(() => {
    const current = currentAppIcon()
    if (current && current !== useStore.getState().settings.appIcon) setSettings({ appIcon: current })
  }, [setSettings])
}

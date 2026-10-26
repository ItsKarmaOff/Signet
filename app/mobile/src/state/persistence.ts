// Reads at launch and writes the store to AsyncStorage with a delay (see packages/shared/src/mobile/persist.ts for the format).
// Writing is best-effort: a storage error must never crash the app. Private tabs are never written.
import AsyncStorage from '@react-native-async-storage/async-storage'
import { AppState as RNAppState } from 'react-native'
import { LEGACY_TABS_KEY, parsePersisted, serializePersisted, STORAGE_KEY } from '@signet/shared/mobile/persist'
import { applyLanguage, t } from '@/lib/i18n'
import { useStore } from './store'
import type { AppState } from './types'

const WRITE_DELAY = 600

const snapshot = (s: AppState) => ({ workspace: s.workspace, history: s.history, bookmarks: s.bookmarks, settings: s.settings, ui: s.ui })

async function load(): Promise<void> {
  let text: string | null = null
  let legacy: string | null = null
  try {
    text = await AsyncStorage.getItem(STORAGE_KEY)
    if (!text) legacy = await AsyncStorage.getItem(LEGACY_TABS_KEY)
  } catch {
    // Unreadable storage: start over from the defaults.
  }
  const loaded = parsePersisted(text, { defaultSpaceName: t('spaces.defaultName'), legacyTabs: legacy })
  applyLanguage(loaded.settings.language)
  useStore.setState({ ...loaded, hydrated: true })
}

function watch(): void {
  let timer: ReturnType<typeof setTimeout> | undefined
  let dirty = false
  const write = () => {
    timer = undefined
    if (!dirty) return
    dirty = false
    AsyncStorage.setItem(STORAGE_KEY, serializePersisted(snapshot(useStore.getState()))).catch(() => {})
  }
  let last = snapshot(useStore.getState())
  useStore.subscribe((s) => {
    if (!s.hydrated) return
    const cur = snapshot(s)
    // The language name follows the setting.
    if (cur.settings.language !== last.settings.language) applyLanguage(cur.settings.language)
    const changed = (Object.keys(cur) as (keyof typeof cur)[]).some((k) => cur[k] !== last[k])
    last = cur
    if (!changed) return
    dirty = true
    if (!timer) timer = setTimeout(write, WRITE_DELAY)
  })
  // When the app goes to the background, write immediately (it may be killed without warning).
  RNAppState.addEventListener('change', (state) => {
    if (state !== 'active') {
      if (timer) clearTimeout(timer)
      write()
    }
  })
}

let started: Promise<void> | null = null

/** Loads the saved state then enables automatic writing. Idempotent: call once from the root. */
export function bootstrapStore(): Promise<void> {
  started ??= load().then(watch)
  return started
}

import type { StateCreator } from 'zustand'
import { DEFAULT_MOBILE_SETTINGS, patchSettings } from '@signet/shared/mobile/settings'
import type { AppState, SettingsSlice } from '../types'

export const createSettingsSlice: StateCreator<AppState, [], [], SettingsSlice> = (set) => ({
  settings: DEFAULT_MOBILE_SETTINGS,
  setSettings: (patch) => set((s) => ({ settings: patchSettings(s.settings, patch) })),
  resetSettings: () => set({ settings: DEFAULT_MOBILE_SETTINGS })
})

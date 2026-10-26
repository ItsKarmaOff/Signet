import type { StateCreator } from 'zustand'
import { DEFAULT_UI } from '@signet/shared/mobile/persist'
import type { AppState, UiSlice } from '../types'

export const createUiSlice: StateCreator<AppState, [], [], UiSlice> = (set) => ({
  ui: DEFAULT_UI,
  hydrated: false,
  setOnboardingDone: (done = true) => set((s) => ({ ui: { ...s.ui, onboardingDone: done } }))
})

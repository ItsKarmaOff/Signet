// Factories and sanitizers of the data model (spaces, tabs). Pure: default (translated) names come from the caller.
import type { Space, SpaceDraft, SpaceTheme, TabInfo } from './types'
import { newId } from './ids'

export function newSpace(draft: { name: string; icon: string; hue: number; theme?: SpaceTheme }): Space {
  return { id: newId(), ...draft, pinned: [], today: [], activeTabId: null }
}

const cleanHue = (h: unknown): number | null => (typeof h === 'number' && Number.isFinite(h) ? ((Math.round(h) % 360) + 360) % 360 : null)

/**
 * Space draft coming from the UI, validated: non-empty name and emoji, hue 0–359, gradient of 2 or 3 hues
 * (otherwise solid color). With a gradient, `hue` is its first hue. `defaultName`: (translated) name of an unnamed space.
 */
export function cleanSpaceDraft(draft: SpaceDraft, defaultName: string): { name: string; icon: string; hue: number; theme?: SpaceTheme } {
  const name = typeof draft?.name === 'string' && draft.name.trim() ? draft.name.trim().slice(0, 40) : defaultName
  const icon = typeof draft?.icon === 'string' && draft.icon.trim() ? draft.icon.trim().slice(0, 16) : '✨'
  const hues = Array.isArray(draft?.theme?.hues)
    ? draft.theme.hues
        .map(cleanHue)
        .filter((h): h is number => h !== null)
        .slice(0, 3)
    : []
  if (hues.length >= 2) {
    const grain = Number(draft.theme?.grain)
    return { name, icon, hue: hues[0], theme: { hues, grain: Number.isFinite(grain) ? Math.min(1, Math.max(0, grain)) : 0 } }
  }
  return { name, icon, hue: cleanHue(draft?.hue) ?? 250 }
}

export function newTab(url: string, title = ''): TabInfo {
  return {
    id: newId(),
    url,
    title,
    loading: false,
    canGoBack: false,
    canGoForward: false,
    loaded: false,
    audible: false,
    muted: false,
    lastActive: Date.now()
  }
}

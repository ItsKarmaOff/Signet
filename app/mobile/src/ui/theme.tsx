// Theme: colors derived from the active space's hue (same computation as desktop, see shared/mobile/color.ts),
// light / dark depending on the setting, and neutral dark in private browsing.
import { createContext, useContext, useEffect, useMemo, type ReactNode } from 'react'
import { Appearance, useColorScheme } from 'react-native'
import { DarkTheme, DefaultTheme, ThemeProvider as NavigationThemeProvider } from 'expo-router'
import { hslToRgb, rgbToHex, spaceGradient, spacePalette, type GradientSpec, type Scheme, type SpacePalette } from '@signet/shared/mobile/color'
import { useStore } from '@/state/store'
import { selectActiveSpace } from '@/state/selectors'

export interface ThemeColors extends SpacePalette {
  text: string
  textSecondary: string
  textTertiary: string
  danger: string
  /** Text on a `danger` background. */
  onDanger: string
  /** Traits fins entre lignes. */
  separator: string
  /** Veil behind a sheet or menu. */
  scrim: string
  /** Translucent background laid over the blur (glass bars). */
  glassTint: string
}

export interface Theme {
  scheme: Scheme
  /** Hue of the space (or private mode) that colors the theme. */
  hue: number
  colors: ThemeColors
  /** Background gradient of the active space, ready for `expo-linear-gradient`. */
  gradient: GradientSpec
  isPrivate: boolean
}

/** Private mode hue: a discreet gray-violet. */
export const PRIVATE_HUE = 285

const NEUTRAL = {
  light: { text: '#1d1d1f', textSecondary: '#5c5c62', textTertiary: '#6c6c73' },
  dark: { text: '#ececf0', textSecondary: '#a5a4ad', textTertiary: '#908f98' }
}
const DANGER = { light: rgbToHex(hslToRgb(2, 0.7, 0.46)), dark: rgbToHex(hslToRgb(2, 0.8, 0.68)) }

/** Theme for a given hue and scheme (also for previewing another space). */
export function buildTheme(
  hue: number,
  scheme: Scheme,
  gradientSource: { hue: number; theme?: { hues: number[]; grain: number } } = { hue },
  isPrivate = false
): Theme {
  const palette = spacePalette(hue, scheme)
  const n = NEUTRAL[scheme]
  return {
    scheme,
    hue,
    isPrivate,
    gradient: spaceGradient(gradientSource, scheme),
    colors: {
      ...palette,
      ...n,
      danger: DANGER[scheme],
      onDanger: scheme === 'dark' ? '#121216' : '#ffffff',
      separator: scheme === 'dark' ? 'rgba(255,255,255,0.10)' : 'rgba(0,0,0,0.10)',
      scrim: scheme === 'dark' ? 'rgba(0,0,0,0.5)' : 'rgba(0,0,0,0.28)',
      glassTint: scheme === 'dark' ? 'rgba(28,28,32,0.5)' : 'rgba(255,255,255,0.55)'
    }
  }
}

const ThemeContext = createContext<Theme | null>(null)

/** Provides the theme to the tree: place under the hydrated store (see `app/_layout.tsx`). */
export function ThemeProvider({ children }: { children: ReactNode }) {
  const system = useColorScheme() === 'dark' ? 'dark' : 'light'
  const pref = useStore((s) => s.settings.theme)
  const isPrivate = useStore((s) => s.workspace.privateMode)
  const space = useStore(selectActiveSpace)
  const scheme: Scheme = isPrivate ? 'dark' : pref === 'system' ? system : pref
  const hue = isPrivate ? PRIVATE_HUE : space.hue
  const themeKey = isPrivate ? '' : JSON.stringify(space.theme ?? null)
  const theme = useMemo(
    () => buildTheme(hue, scheme, isPrivate ? { hue } : { hue: space.hue, theme: space.theme }, isPrivate),
    // eslint-disable-next-line react-hooks/exhaustive-deps -- `themeKey` summarizes `space.theme`
    [hue, scheme, isPrivate, themeKey]
  )
  // The system must know whether the app is light or dark, otherwise iOS draws its own elements (glass bubbles of the
  // headers, keyboard, alerts) in the other mode: a light, unreadable "OK" button on a dark screen. "System" without
  // private browsing lets iOS follow the device setting.
  const override = pref === 'system' && !isPrivate ? 'unspecified' : scheme
  useEffect(() => {
    Appearance.setColorScheme(override)
  }, [override])
  // Navigation theme (native headers), aligned with ours.
  const navigation = useMemo(() => {
    const base = scheme === 'dark' ? DarkTheme : DefaultTheme
    const c = theme.colors
    return {
      ...base,
      colors: { ...base.colors, primary: c.accent, background: c.background, card: c.background, text: c.text, border: c.separator }
    }
  }, [scheme, theme])
  return (
    <ThemeContext.Provider value={theme}>
      <NavigationThemeProvider value={navigation}>{children}</NavigationThemeProvider>
    </ThemeContext.Provider>
  )
}

export function useTheme(): Theme {
  const theme = useContext(ThemeContext)
  if (!theme) throw new Error('useTheme must be used under <ThemeProvider>')
  return theme
}

/** Shortcut: the current theme's colors. */
export const useColors = (): ThemeColors => useTheme().colors

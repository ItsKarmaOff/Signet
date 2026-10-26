// Design tokens, inherited from the desktop's (styles/tokens.css): 4-pt grid, soft radii, weights per role.
// Colors, on the other hand, depend on the space and theme: see `theme.tsx`.
import { Platform, type TextStyle, type ViewStyle } from 'react-native'

/** Espacements (grille de 4). */
export const space = { xxs: 2, xs: 4, sm: 8, md: 12, lg: 16, xl: 20, xxl: 24, xxxl: 32, huge: 48 } as const

/** Side margin of screens. */
export const gutter = 16

/** Radii: each nested level subtracts the spacing that separates it from its parent. */
export const radius = {
  /** Grandes surfaces (feuilles, cartes d'onglets). */
  card: 22,
  /** Groupes de liste, barre flottante. */
  group: 16,
  /** Buttons, fields. */
  control: 14,
  /** Vignettes, favicons. */
  tile: 10,
  small: 6,
  pill: 999
} as const

/** Minimum size of a touch target (Apple: 44 pt). */
export const hit = 44

const fontFamily = Platform.select({ ios: undefined, default: 'sans-serif' })

type Variant = { fontSize: number; lineHeight: number; fontWeight: TextStyle['fontWeight']; letterSpacing?: number }

/** Type scale (system fonts: SF Pro on iOS, Roboto on Android). Sizes from 11 to 34. */
export const type = {
  largeTitle: { fontSize: 34, lineHeight: 41, fontWeight: '700', letterSpacing: 0.37 },
  title1: { fontSize: 28, lineHeight: 34, fontWeight: '700', letterSpacing: 0.36 },
  title2: { fontSize: 22, lineHeight: 28, fontWeight: '700', letterSpacing: 0.35 },
  title3: { fontSize: 20, lineHeight: 25, fontWeight: '600', letterSpacing: 0.38 },
  headline: { fontSize: 17, lineHeight: 22, fontWeight: '600', letterSpacing: -0.41 },
  body: { fontSize: 17, lineHeight: 22, fontWeight: '400', letterSpacing: -0.41 },
  callout: { fontSize: 16, lineHeight: 21, fontWeight: '400', letterSpacing: -0.32 },
  subhead: { fontSize: 15, lineHeight: 20, fontWeight: '400', letterSpacing: -0.24 },
  footnote: { fontSize: 13, lineHeight: 18, fontWeight: '400', letterSpacing: -0.08 },
  caption: { fontSize: 12, lineHeight: 16, fontWeight: '400' },
  caption2: { fontSize: 11, lineHeight: 13, fontWeight: '500', letterSpacing: 0.07 }
} as const satisfies Record<string, Variant>

export type TypeVariant = keyof typeof type

export const fontStyle = (v: TypeVariant): TextStyle => ({ fontFamily, ...type[v] })

/** Shadows: the card floats slightly, the floating layer (bar, menu) higher. */
export const shadow = {
  card: Platform.select<ViewStyle>({
    ios: { shadowColor: '#000', shadowOpacity: 0.08, shadowRadius: 8, shadowOffset: { width: 0, height: 3 } },
    default: { elevation: 2 }
  }) as ViewStyle,
  pop: Platform.select<ViewStyle>({
    ios: { shadowColor: '#000', shadowOpacity: 0.22, shadowRadius: 24, shadowOffset: { width: 0, height: 10 } },
    default: { elevation: 10 }
  }) as ViewStyle,
  knob: Platform.select<ViewStyle>({
    ios: { shadowColor: '#000', shadowOpacity: 0.25, shadowRadius: 2, shadowOffset: { width: 0, height: 1 } },
    default: { elevation: 2 }
  }) as ViewStyle
} as const

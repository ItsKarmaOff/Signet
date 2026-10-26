import { Text as RNText, type TextProps as RNTextProps } from 'react-native'
import { useColors } from './theme'
import { fontStyle, type TypeVariant } from './tokens'

export interface TextProps extends RNTextProps {
  /** Typographic role (default `body`). */
  variant?: TypeVariant
  /** Color: a theme role, or a raw color. */
  color?: 'text' | 'secondary' | 'tertiary' | 'accent' | 'danger' | 'onAccent' | (string & {})
  /** Forced weight (otherwise the role's). */
  weight?: '400' | '500' | '600' | '700'
  align?: 'left' | 'center' | 'right'
}

/** App text: system font, type scale and theme colors. */
export function Text({ variant = 'body', color = 'text', weight, align, style, ...rest }: TextProps) {
  const c = useColors()
  const resolved =
    color === 'text'
      ? c.text
      : color === 'secondary'
        ? c.textSecondary
        : color === 'tertiary'
          ? c.textTertiary
          : color === 'accent'
            ? c.accent
            : color === 'danger'
              ? c.danger
              : color === 'onAccent'
                ? c.onAccent
                : color
  return <RNText {...rest} style={[fontStyle(variant), { color: resolved }, weight && { fontWeight: weight }, align && { textAlign: align }, style]} />
}

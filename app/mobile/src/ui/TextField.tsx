import { forwardRef, useState, type ComponentRef, type ReactNode } from 'react'
import { StyleSheet, TextInput, View, type TextInputProps } from 'react-native'
import { IconButton } from './IconButton'
import { Search, X } from './icons'
import { useColors, useTheme } from './theme'
import { Text } from './Text'
import { fontStyle, radius, space } from './tokens'

/** Handle of a field (`ref.current?.focus()`). */
export type TextInputHandle = ComponentRef<typeof TextInput>

export interface TextFieldProps extends Omit<TextInputProps, 'style'> {
  /** Label above the field. */
  label?: string
  /** Error message below the field (the outline turns red). */
  error?: string
  /** Element on the right inside the field (button, text…). */
  right?: ReactNode
}

/**
 * Input field. Fixed height (`minHeight`): never put `flex: 1` on a field in a column,
 * it collapses to 0 height (bug from the first skeleton).
 */
export const TextField = forwardRef<TextInputHandle, TextFieldProps>(function TextField(
  { label, error, right, multiline, onFocus, onBlur, accessibilityLabel, ...rest },
  ref
) {
  const c = useColors()
  const { scheme } = useTheme()
  const [focused, setFocused] = useState(false)
  return (
    <View style={styles.wrap}>
      {label ? (
        <Text variant="footnote" color="secondary" style={styles.label}>
          {label}
        </Text>
      ) : null}
      <View
        style={[
          styles.field,
          multiline && styles.multiline,
          { backgroundColor: c.surface2, borderColor: error ? c.danger : focused ? c.accent : 'transparent' }
        ]}
      >
        <TextInput
          ref={ref}
          multiline={multiline}
          placeholderTextColor={c.textTertiary}
          keyboardAppearance={scheme}
          selectionColor={c.accent}
          accessibilityLabel={accessibilityLabel ?? label}
          onFocus={(e) => {
            setFocused(true)
            onFocus?.(e)
          }}
          onBlur={(e) => {
            setFocused(false)
            onBlur?.(e)
          }}
          {...rest}
          style={[styles.input, fontStyle('body'), { color: c.text }, multiline && styles.inputMulti]}
        />
        {right}
      </View>
      {error ? (
        <Text variant="footnote" color="danger" style={styles.label}>
          {error}
        </Text>
      ) : null}
    </View>
  )
})

export interface SearchFieldProps extends Omit<TextInputProps, 'style' | 'onChange'> {
  value: string
  onChangeText: (text: string) => void
  /** Label of the clear button (accessibility). */
  clearLabel: string
  /** Field background: `surface2` (default) on a screen, `surface` in a card. */
  tone?: 'surface' | 'surface2'
}

/** Rounded search field: magnifier, clear button when there is text. */
export const SearchField = forwardRef<TextInputHandle, SearchFieldProps>(function SearchField(
  { value, onChangeText, clearLabel, tone = 'surface2', ...rest },
  ref
) {
  const c = useColors()
  const { scheme } = useTheme()
  return (
    <View style={[styles.search, { backgroundColor: c[tone] }]}>
      <Search size={18} color={c.textTertiary} strokeWidth={2} />
      <TextInput
        ref={ref}
        value={value}
        onChangeText={onChangeText}
        placeholderTextColor={c.textTertiary}
        keyboardAppearance={scheme}
        selectionColor={c.accent}
        autoCapitalize="none"
        autoCorrect={false}
        returnKeyType="search"
        {...rest}
        style={[styles.searchInput, fontStyle('body'), { color: c.text }]}
      />
      {value ? <IconButton icon={X} label={clearLabel} size={16} tone="muted" onPress={() => onChangeText('')} /> : null}
    </View>
  )
})

const styles = StyleSheet.create({
  wrap: { alignSelf: 'stretch' },
  label: { marginBottom: space.xs, marginHorizontal: space.xs },
  field: { minHeight: 48, borderRadius: radius.control, borderWidth: 1.5, paddingHorizontal: space.lg, flexDirection: 'row', alignItems: 'center' },
  multiline: { alignItems: 'flex-start' },
  input: { flexGrow: 1, flexShrink: 1, minHeight: 44, padding: 0 },
  inputMulti: { minHeight: 96, paddingTop: space.md, textAlignVertical: 'top' },
  search: { height: 44, borderRadius: radius.control, paddingLeft: space.md, flexDirection: 'row', alignItems: 'center', gap: space.sm },
  searchInput: { flexGrow: 1, flexShrink: 1, height: 44, padding: 0 }
})

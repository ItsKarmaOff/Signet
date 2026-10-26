// iOS Settings-style lists: rounded sections, rows separated by a thin line, icon in a colored badge.
import { Children, isValidElement, type ReactNode } from 'react'
import { Platform, StyleSheet, Switch, View } from 'react-native'
import { ChevronRight, Check, type LucideIcon } from './icons'
import { PressableScale } from './motion'
import { useColors } from './theme'
import { Text } from './Text'
import { gutter, radius, space } from './tokens'

export interface ListSectionProps {
  /** Header in discreet small caps. */
  title?: string
  /** Explanatory text below the section. */
  footer?: string
  children?: ReactNode
}

/** Group of rows in a rounded card. `false`/`null` children are ignored. */
export function ListSection({ title, footer, children }: ListSectionProps) {
  const c = useColors()
  const rows = Children.toArray(children).filter(isValidElement)
  if (rows.length === 0) return null
  return (
    <View style={styles.section}>
      {title ? (
        <Text variant="footnote" color="secondary" style={styles.header} accessibilityRole="header">
          {title.toUpperCase()}
        </Text>
      ) : null}
      <View style={[styles.card, { backgroundColor: c.surface }]}>
        {rows.map((row, i) => (
          <View key={row.key ?? i}>
            {i > 0 ? <View style={[styles.separator, { backgroundColor: c.separator }]} /> : null}
            {row}
          </View>
        ))}
      </View>
      {footer ? (
        <Text variant="footnote" color="secondary" style={styles.footer}>
          {footer}
        </Text>
      ) : null}
    </View>
  )
}

export interface ListRowProps {
  title: string
  subtitle?: string
  /** Icon in a rounded badge (Settings style). */
  icon?: LucideIcon
  /** Badge color (default: the space's accent). */
  iconColor?: string
  /** Free content on the left (favicon…), in place of `icon`. */
  left?: ReactNode
  /** Current value shown on the right, in gray. */
  value?: string
  /** Free content on the right (replaces `value`). */
  right?: ReactNode
  /** Chevron: true by default if the row is pressable. */
  chevron?: boolean
  /** Checkmark (row chosen in a list of options). */
  selected?: boolean
  destructive?: boolean
  disabled?: boolean
  onPress?: () => void
  onLongPress?: () => void
  testID?: string
}

export function ListRow({
  title,
  subtitle,
  icon: Icon,
  iconColor,
  left,
  value,
  right,
  chevron,
  selected,
  destructive,
  disabled,
  onPress,
  onLongPress,
  testID
}: ListRowProps) {
  const c = useColors()
  const showChevron = chevron ?? (!!onPress && selected === undefined && !right)
  const content = (
    <View style={styles.row}>
      {left ??
        (Icon ? (
          <View style={[styles.iconBox, { backgroundColor: iconColor ?? (destructive ? c.danger : c.accent) }]}>
            <Icon size={17} color="#ffffff" strokeWidth={2} />
          </View>
        ) : null)}
      <View style={styles.texts}>
        <Text variant="body" color={destructive ? 'danger' : 'text'} numberOfLines={1}>
          {title}
        </Text>
        {subtitle ? (
          <Text variant="footnote" color="secondary" numberOfLines={3}>
            {subtitle}
          </Text>
        ) : null}
      </View>
      {right ??
        (value ? (
          <Text variant="body" color="secondary" numberOfLines={1} style={styles.value}>
            {value}
          </Text>
        ) : null)}
      {selected ? <Check size={20} color={c.accent} strokeWidth={2.4} /> : null}
      {showChevron ? <ChevronRight size={18} color={c.textTertiary} strokeWidth={2} /> : null}
    </View>
  )
  if (!onPress && !onLongPress) return <View testID={testID}>{content}</View>
  return (
    <PressableScale
      accessibilityRole="button"
      accessibilityLabel={title}
      accessibilityState={{ disabled: !!disabled, selected: !!selected }}
      testID={testID}
      onPress={onPress}
      onLongPress={onLongPress}
      disabled={disabled}
      scaleTo={0.985}
      style={disabled && styles.disabled}
    >
      {content}
    </PressableScale>
  )
}

export interface SwitchRowProps {
  title: string
  subtitle?: string
  icon?: LucideIcon
  iconColor?: string
  value: boolean
  onValueChange: (value: boolean) => void
  disabled?: boolean
  testID?: string
}

/**
 * Switch in the app's colors. On Android, the thumb and track otherwise take the default Material colors
 * (turquoise thumb on purple track): white thumb, accent or gray track.
 */
export function Toggle(props: { value: boolean; onValueChange: (value: boolean) => void; disabled?: boolean; accessibilityLabel?: string }) {
  const c = useColors()
  const android = Platform.OS === 'android'
  return <Switch {...props} trackColor={{ true: c.accent, false: android ? c.textTertiary : undefined }} thumbColor={android ? '#ffffff' : undefined} />
}

/** Row with a switch. */
export function SwitchRow({ title, subtitle, icon, iconColor, value, onValueChange, disabled, testID }: SwitchRowProps) {
  return (
    <ListRow
      title={title}
      subtitle={subtitle}
      icon={icon}
      iconColor={iconColor}
      disabled={disabled}
      testID={testID}
      right={<Toggle value={value} onValueChange={onValueChange} disabled={disabled} accessibilityLabel={title} />}
    />
  )
}

const styles = StyleSheet.create({
  section: { marginHorizontal: gutter, marginBottom: space.xxl },
  header: { marginLeft: space.lg, marginBottom: space.sm, letterSpacing: 0.2 },
  footer: { marginHorizontal: space.lg, marginTop: space.sm },
  card: { borderRadius: radius.group, overflow: 'hidden' },
  separator: { height: StyleSheet.hairlineWidth, marginLeft: space.lg },
  row: { minHeight: 48, paddingHorizontal: space.lg, paddingVertical: space.sm, flexDirection: 'row', alignItems: 'center', gap: space.md },
  iconBox: { width: 29, height: 29, borderRadius: 7, alignItems: 'center', justifyContent: 'center' },
  texts: { flexShrink: 1, flexGrow: 1 },
  value: { flexShrink: 1, maxWidth: '50%' },
  disabled: { opacity: 0.45 }
})

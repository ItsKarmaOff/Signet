// Space editor (sheet): name, emoji, color or gradient with preview, order and deletion.
// `/spaces/edit/new` creates a space; `/spaces/edit/<id>` edits that one. Changes apply with "OK" / "Create".
import { useMemo, useState } from 'react'
import { Alert, Pressable, StyleSheet, Text as RNText, View } from 'react-native'
import { LinearGradient } from 'expo-linear-gradient'
import { useLocalSearchParams, useRouter } from 'expo-router'
import { hslToRgb, rgbToHex, withAlpha } from '@signet/shared/mobile/color'
import { MAX_SPACES, canAddSpace, canRemoveSpace, spaceTabIds } from '@signet/shared/mobile/workspace'
import { haptic } from '@/lib/haptics'
import { useT } from '@/lib/i18n'
import { useStore } from '@/state'
import { ListRow, ListSection, PressableScale, Sheet, SwitchRow, Text, TextField, buildTheme, gutter, icons, radius, space, useTheme } from '@/ui'
import { SPACE_EMOJIS } from './emojis'
import { HueSlider } from './HueSlider'
import { SpaceBadge } from './SpaceBadge'

/** Hues offered with one tap (the slider allows all the others). */
const SWATCHES = [0, 22, 45, 90, 140, 172, 200, 225, 250, 275, 310, 338]

export function EditSpaceSheet() {
  const t = useT()
  const router = useRouter()
  const { colors: c, scheme } = useTheme()
  const { id } = useLocalSearchParams<{ id: string }>()
  const creating = id === 'new'
  const existing = useStore((s) => s.workspace.spaces.find((x) => x.id === id))
  const spaces = useStore((s) => s.workspace.spaces)
  const ws = useStore((s) => s.workspace)
  const [name, setName] = useState(existing?.name ?? '')
  const [icon, setIcon] = useState(existing?.icon ?? SPACE_EMOJIS[spaces.length % SPACE_EMOJIS.length])
  const [hue, setHue] = useState(existing?.theme?.hues[0] ?? existing?.hue ?? SWATCHES[(spaces.length * 5) % SWATCHES.length])
  const [hue2, setHue2] = useState(existing?.theme?.hues[1] ?? (hue + 60) % 360)
  const [gradient, setGradient] = useState((existing?.theme?.hues.length ?? 0) >= 2)

  const draft = useMemo(
    () => ({ name, icon, hue, ...(gradient ? { theme: { hues: [hue, hue2], grain: existing?.theme?.grain ?? 0 } } : {}) }),
    [name, icon, hue, hue2, gradient, existing?.theme?.grain]
  )

  if (!creating && !existing) {
    // The space is gone (deleted elsewhere): nothing to edit.
    queueMicrotask(() => router.dismiss())
    return null
  }

  const index = existing ? spaces.findIndex((s) => s.id === existing.id) : -1
  const full = creating && !canAddSpace(ws)
  const save = () => {
    if (creating) {
      if (!full) useStore.getState().addSpace(draft)
    } else if (existing) useStore.getState().editSpace(existing.id, draft)
    haptic('success')
    router.dismiss()
  }

  const remove = () => {
    if (!existing) return
    const count = spaceTabIds(existing).length
    Alert.alert(t('spaces.deleteTitle'), count > 0 ? t('spaces.deleteMessage', { count }) : undefined, [
      { text: t('common.cancel'), style: 'cancel' },
      {
        text: t('common.delete'),
        style: 'destructive',
        onPress: () => {
          useStore.getState().removeSpace(existing.id)
          haptic('success')
          router.dismiss()
        }
      }
    ])
  }

  return (
    <Sheet title={creating ? t('spaces.new') : t('spaces.edit')} doneLabel={creating ? t('spaces.create') : t('common.done')} onDone={save}>
      <View style={styles.pad}>
        {full ? (
          <Text variant="footnote" color="danger" style={styles.notice}>
            {t('spaces.maxReached', { count: MAX_SPACES })}
          </Text>
        ) : null}
        <Preview draft={draft} name={name} scheme={scheme} />
        <TextField
          label={t('spaces.name')}
          value={name}
          onChangeText={setName}
          placeholder={t('spaces.namePlaceholder')}
          maxLength={40}
          returnKeyType="done"
          autoCapitalize="sentences"
        />
      </View>

      <Text variant="footnote" color="secondary" style={styles.label}>
        {t('spaces.icon').toUpperCase()}
      </Text>
      <View style={[styles.emojiGrid, { backgroundColor: c.surface }]}>
        {SPACE_EMOJIS.map((e) => (
          <Pressable
            key={e}
            accessibilityRole="button"
            accessibilityLabel={e}
            accessibilityState={{ selected: e === icon }}
            onPress={() => {
              haptic('selection')
              setIcon(e)
            }}
            style={[styles.emoji, e === icon && { backgroundColor: withAlpha(c.accent, 0.18), borderColor: c.accent }]}
          >
            <RNText style={styles.emojiText}>{e}</RNText>
          </Pressable>
        ))}
      </View>

      <Text variant="footnote" color="secondary" style={styles.label}>
        {t('spaces.color').toUpperCase()}
      </Text>
      <View style={[styles.colorCard, { backgroundColor: c.surface }]}>
        <View style={styles.swatches}>
          {SWATCHES.map((h) => {
            const color = rgbToHex(hslToRgb(h, 0.7, 0.62))
            const selected = Math.abs(h - hue) < 6
            return (
              <PressableScale
                key={h}
                accessibilityRole="button"
                accessibilityLabel={`${t('spaces.color')} ${h}`}
                accessibilityState={{ selected }}
                scaleTo={0.88}
                onPress={() => {
                  haptic('selection')
                  setHue(h)
                }}
                style={[styles.swatch, { backgroundColor: color, borderColor: selected ? c.text : 'transparent' }]}
              />
            )
          })}
        </View>
        <HueSlider value={hue} onChange={setHue} label={gradient ? t('spaces.color1') : t('spaces.color')} />
        {gradient ? <HueSlider value={hue2} onChange={setHue2} label={t('spaces.color2')} /> : null}
      </View>
      <ListSection footer={t('spaces.gradientHint')}>
        <SwitchRow
          title={t('spaces.gradient')}
          value={gradient}
          onValueChange={(on) => {
            haptic('selection')
            setGradient(on)
            if (on) setHue2((hue + 60) % 360)
          }}
        />
      </ListSection>

      {existing && spaces.length > 1 ? (
        <ListSection title={t('spaces.order')}>
          <ListRow
            icon={icons.ArrowUp}
            iconColor={c.textSecondary}
            title={t('spaces.moveUp')}
            chevron={false}
            disabled={index <= 0}
            onPress={() => useStore.getState().moveSpace(existing.id, index - 1)}
          />
          <ListRow
            icon={icons.ArrowDown}
            iconColor={c.textSecondary}
            title={t('spaces.moveDown')}
            chevron={false}
            disabled={index >= spaces.length - 1}
            onPress={() => useStore.getState().moveSpace(existing.id, index + 1)}
          />
        </ListSection>
      ) : null}

      {existing ? (
        <ListSection footer={canRemoveSpace(ws) ? undefined : t('spaces.deleteLast')}>
          <ListRow title={t('spaces.delete')} destructive chevron={false} disabled={!canRemoveSpace(ws)} onPress={remove} />
        </ListSection>
      ) : null}
    </Sheet>
  )
}

/** Preview: the space background (tinted gradient as on desktop), a card, an accent button. */
function Preview({
  draft,
  name,
  scheme
}: {
  draft: { icon: string; hue: number; theme?: { hues: number[]; grain: number } }
  name: string
  scheme: 'light' | 'dark'
}) {
  const t = useT()
  const theme = useMemo(() => buildTheme(draft.hue, scheme, { hue: draft.hue, theme: draft.theme }), [draft.hue, draft.theme, scheme])
  const { colors: p, gradient } = theme
  return (
    <View style={[styles.preview, { backgroundColor: p.background, borderColor: p.border }]} accessibilityLabel={t('spaces.preview')}>
      <LinearGradient
        colors={gradient.colors.map((col) => withAlpha(col, scheme === 'dark' ? 0.3 : 0.24)) as [string, string, ...string[]]}
        start={gradient.start}
        end={gradient.end}
        style={StyleSheet.absoluteFill}
      />
      <View style={styles.previewRow}>
        <SpaceBadge icon={draft.icon} hue={draft.hue} theme={draft.theme} size={40} />
        <Text variant="headline" numberOfLines={1} style={{ flex: 1, color: p.text }}>
          {name || t('spaces.namePlaceholder')}
        </Text>
      </View>
      <View style={styles.previewRow}>
        {[0, 1].map((i) => (
          <View key={i} style={[styles.previewCard, { backgroundColor: p.surface }]}>
            <View style={[styles.previewDot, { backgroundColor: p.accent }]} />
            <View style={[styles.previewLine, { backgroundColor: p.textTertiary }]} />
          </View>
        ))}
        <View style={[styles.previewPill, { backgroundColor: p.accent }]} />
      </View>
    </View>
  )
}

const styles = StyleSheet.create({
  pad: { paddingHorizontal: gutter, gap: space.lg, marginBottom: space.lg },
  notice: { textAlign: 'center' },
  label: { marginLeft: gutter + space.lg, marginBottom: space.sm, letterSpacing: 0.2 },
  emojiGrid: {
    marginHorizontal: gutter,
    marginBottom: space.xxl,
    borderRadius: radius.group,
    padding: space.sm,
    flexDirection: 'row',
    flexWrap: 'wrap',
    justifyContent: 'center'
  },
  emoji: {
    width: '10%',
    minWidth: 34,
    aspectRatio: 1,
    borderRadius: radius.tile,
    borderWidth: 1.5,
    borderColor: 'transparent',
    alignItems: 'center',
    justifyContent: 'center'
  },
  emojiText: { fontSize: 22 },
  colorCard: { marginHorizontal: gutter, marginBottom: space.lg, borderRadius: radius.group, padding: space.md, gap: space.xs },
  swatches: { flexDirection: 'row', flexWrap: 'wrap', justifyContent: 'space-between', gap: space.sm, marginBottom: space.xs },
  swatch: { width: 34, height: 34, borderRadius: 17, borderWidth: 3 },
  preview: { borderRadius: radius.card, borderWidth: StyleSheet.hairlineWidth, padding: space.lg, gap: space.md, overflow: 'hidden' },
  previewRow: { flexDirection: 'row', alignItems: 'center', gap: space.md },
  previewCard: { flex: 1, height: 44, borderRadius: radius.tile, flexDirection: 'row', alignItems: 'center', gap: space.sm, paddingHorizontal: space.md },
  previewDot: { width: 14, height: 14, borderRadius: 4, opacity: 0.8 },
  previewLine: { flex: 1, height: 6, borderRadius: 3, opacity: 0.45 },
  previewPill: { width: 44, height: 28, borderRadius: radius.pill }
})

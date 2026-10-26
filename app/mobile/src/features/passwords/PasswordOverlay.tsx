// What passwords add to a tab's view: the "Save password?" card and the key button that fills.
import { StyleSheet, View } from 'react-native'
import Animated, { FadeInDown, FadeOutDown } from 'react-native-reanimated'
import { siteOf } from '@signet/shared/site'
import type { Credential } from '@signet/shared/mobile/passwords'
import { useT } from '@/lib/i18n'
import { ActionSheet, Button, GlassBar, PressableScale, Text, gutter, icons, radius, space, useColors, useReduceMotion } from '@/ui'

export interface SavePrompt {
  origin: string
  username: string
  password: string
  /** Existing login to update (same site, same username, different password). */
  updateId?: string
}

interface Props {
  prompt: SavePrompt | null
  onAnswer: (what: 'save' | 'later' | 'never') => void
  canFill: boolean
  onFill: () => void
  picker: Credential[] | null
  /** `null`: sheet dismissed without a choice. */
  onPick: (c: Credential | null) => void
}

/** Above the expanded bottom bar (it covers the page; the page only keeps room for the collapsed bar). */
const ABOVE_BAR = 74

export function PasswordOverlay({ prompt, onAnswer, canFill, onFill, picker, onPick }: Props) {
  const t = useT()
  const c = useColors()
  const reduce = useReduceMotion()
  const site = prompt ? siteOf(prompt.origin) || prompt.origin : ''
  return (
    <>
      <View style={[styles.layer, { bottom: ABOVE_BAR }]} pointerEvents="box-none">
        {prompt ? (
          <Animated.View
            entering={reduce ? undefined : FadeInDown.duration(200)}
            exiting={reduce ? undefined : FadeOutDown.duration(150)}
            style={styles.cardWrap}
          >
            <GlassBar floating intensity={100} style={[styles.card, { backgroundColor: c.surface }]}>
              <View style={styles.head}>
                <View style={[styles.glyph, { backgroundColor: c.selection }]}>
                  <icons.Key size={20} color={c.accent} strokeWidth={2} />
                </View>
                <View style={styles.headText}>
                  <Text variant="headline" numberOfLines={2}>
                    {t(prompt.updateId ? 'passwords.updateTitle' : 'passwords.saveTitle', { site })}
                  </Text>
                  <Text variant="footnote" color="secondary" numberOfLines={1}>
                    {prompt.username || t('passwords.noUsername')}
                  </Text>
                </View>
              </View>
              <View style={styles.actions}>
                <Button title={t(prompt.updateId ? 'passwords.updateAction' : 'passwords.saveAction')} onPress={() => onAnswer('save')} fullWidth />
                <View style={styles.secondary}>
                  <Button title={t('passwords.notNow')} variant="secondary" onPress={() => onAnswer('later')} style={styles.half} />
                  <Button title={t('passwords.neverForSite')} variant="secondary" onPress={() => onAnswer('never')} style={styles.half} />
                </View>
              </View>
            </GlassBar>
          </Animated.View>
        ) : canFill ? (
          <Animated.View
            entering={reduce ? undefined : FadeInDown.duration(160)}
            exiting={reduce ? undefined : FadeOutDown.duration(120)}
            style={styles.chipWrap}
          >
            <PressableScale
              accessibilityRole="button"
              accessibilityLabel={t('passwords.fill')}
              haptic="light"
              onPress={onFill}
              style={[styles.chip, { backgroundColor: c.accent }]}
            >
              <icons.Key size={16} color={c.onAccent} strokeWidth={2.2} />
              <Text variant="subhead" weight="600" style={{ color: c.onAccent }}>
                {t('passwords.fill')}
              </Text>
            </PressableScale>
          </Animated.View>
        ) : null}
      </View>
      <ActionSheet
        visible={!!picker}
        onClose={() => onPick(null)}
        title={t('passwords.chooseAccount')}
        cancelLabel={t('common.cancel')}
        actions={(picker ?? []).map((cred) => ({
          label: `${cred.username || t('passwords.noUsername')}${siteOf(cred.origin) && cred.origin !== '' ? ` · ${siteOf(cred.origin)}` : ''}`,
          onPress: () => onPick(cred)
        }))}
      />
    </>
  )
}

const styles = StyleSheet.create({
  layer: { position: 'absolute', left: 0, right: 0 },
  cardWrap: { paddingHorizontal: gutter - 4 },
  card: { padding: space.md, gap: space.md },
  head: { flexDirection: 'row', alignItems: 'center', gap: space.md },
  glyph: { width: 40, height: 40, borderRadius: radius.tile, alignItems: 'center', justifyContent: 'center' },
  headText: { flex: 1 },
  actions: { gap: space.sm },
  secondary: { flexDirection: 'row', gap: space.sm },
  half: { flex: 1 },
  chipWrap: { alignItems: 'flex-end', paddingHorizontal: gutter },
  chip: { flexDirection: 'row', alignItems: 'center', gap: space.xs, height: 40, paddingHorizontal: space.lg, borderRadius: radius.pill }
})

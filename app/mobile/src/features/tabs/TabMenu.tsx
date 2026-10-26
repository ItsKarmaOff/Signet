// Tab menu (long-press on a card): pin, favorite, duplicate, move to a space, close.
// A custom sheet rather than a native menu: Expo Go has no context menu, and this one follows the space's theme.
import { useState } from 'react'
import { Pressable, StyleSheet, View } from 'react-native'
import Animated, { FadeIn, FadeOut, SlideInDown, SlideOutDown } from 'react-native-reanimated'
import { useSafeAreaInsets } from 'react-native-safe-area-context'
import { siteOf } from '@signet/shared/site'
import { tabLocation, type MobileSpace, type Workspace } from '@signet/shared/mobile/workspace'
import { haptic } from '@/lib/haptics'
import { useT } from '@/lib/i18n'
import { Favicon, ListRow, ListSection, Text, icons, radius, shadow, space, springs, useColors, useReduceMotion } from '@/ui'
import { SpaceBadge } from '../spaces'

export interface TabMenuProps {
  tabId: string | null
  workspace: Workspace
  onDismiss: () => void
  onPin: (id: string, pinned: boolean) => void
  onFavorite: (id: string, favorite: boolean) => void
  onDuplicate: (id: string) => void
  onMove: (id: string, spaceId: string) => void
  onCloseTab: (id: string) => void
}

export function TabMenu({ tabId, workspace, onDismiss, onPin, onFavorite, onDuplicate, onMove, onCloseTab }: TabMenuProps) {
  const reduce = useReduceMotion()
  const tab = tabId ? workspace.tabs[tabId] : undefined
  if (!tabId || !tab) return null
  return (
    <View style={StyleSheet.absoluteFill} pointerEvents="box-none">
      <MenuBody
        key={tabId}
        tabId={tabId}
        workspace={workspace}
        reduce={reduce}
        onDismiss={onDismiss}
        onPin={onPin}
        onFavorite={onFavorite}
        onDuplicate={onDuplicate}
        onMove={onMove}
        onCloseTab={onCloseTab}
      />
    </View>
  )
}

function MenuBody({
  tabId,
  workspace,
  reduce,
  onDismiss,
  onPin,
  onFavorite,
  onDuplicate,
  onMove,
  onCloseTab
}: Omit<TabMenuProps, 'tabId'> & { tabId: string; reduce: boolean }) {
  const t = useT()
  const c = useColors()
  const insets = useSafeAreaInsets()
  const [view, setView] = useState<'main' | 'spaces'>('main')
  const tab = workspace.tabs[tabId]
  const loc = tabLocation(workspace, tabId)
  if (!tab || !loc) return null
  const isPrivate = loc.kind === 'private'
  const zone = loc.kind === 'space' ? loc.zone : 'today'
  const here = loc.kind === 'space' ? loc.spaceId : ''
  const others = workspace.spaces.filter((s) => s.id !== here)
  const act = (fn: () => void) => () => {
    haptic('light')
    onDismiss()
    fn()
  }
  return (
    <>
      <Animated.View
        entering={reduce ? undefined : FadeIn.duration(140)}
        exiting={reduce ? undefined : FadeOut.duration(120)}
        style={[StyleSheet.absoluteFill, { backgroundColor: c.scrim }]}
      >
        <Pressable style={StyleSheet.absoluteFill} onPress={onDismiss} accessibilityRole="button" accessibilityLabel={t('common.close')} />
      </Animated.View>
      <Animated.View
        entering={reduce ? undefined : SlideInDown.springify().damping(springs.sheet.damping).stiffness(springs.sheet.stiffness).overshootClamping(1)}
        exiting={reduce ? undefined : SlideOutDown.duration(160)}
        style={[styles.sheet, shadow.pop, { backgroundColor: c.background, paddingBottom: Math.max(insets.bottom, space.lg) }]}
      >
        <View style={[styles.grabber, { backgroundColor: c.separator }]} />
        <View style={styles.head}>
          <Favicon uri={tab.favicon} url={tab.url} size={30} />
          <View style={styles.headText}>
            <Text variant="headline" numberOfLines={1}>
              {tab.customTitle || tab.title || siteOf(tab.url) || t('browser.newTabTitle')}
            </Text>
            {tab.url ? (
              <Text variant="footnote" color="secondary" numberOfLines={1}>
                {siteOf(tab.url)}
              </Text>
            ) : null}
          </View>
        </View>
        {view === 'main' ? (
          <>
            {!isPrivate ? (
              <ListSection>
                <ListRow
                  icon={zone === 'pinned' ? icons.PinOff : icons.Pin}
                  title={zone === 'pinned' ? t('tabs.unpin') : t('tabs.pin')}
                  chevron={false}
                  onPress={act(() => onPin(tabId, zone !== 'pinned'))}
                />
                <ListRow
                  icon={zone === 'favorites' ? icons.StarOff : icons.Star}
                  iconColor="#e0a100"
                  title={zone === 'favorites' ? t('tabs.unfavorite') : t('tabs.favorite')}
                  chevron={false}
                  onPress={act(() => onFavorite(tabId, zone !== 'favorites'))}
                />
              </ListSection>
            ) : null}
            <ListSection>
              <ListRow icon={icons.Copy} iconColor={c.textSecondary} title={t('tabs.duplicate')} chevron={false} onPress={act(() => onDuplicate(tabId))} />
              {!isPrivate && others.length > 0 ? (
                <ListRow icon={icons.Layers} iconColor={c.textSecondary} title={t('tabs.moveToSpace')} onPress={() => setView('spaces')} />
              ) : null}
              <ListRow icon={icons.X} iconColor={c.danger} title={t('tabs.close')} destructive chevron={false} onPress={act(() => onCloseTab(tabId))} />
            </ListSection>
          </>
        ) : (
          <ListSection title={t('tabs.moveToSpace')}>
            {others.map((s: MobileSpace) => (
              <ListRow
                key={s.id}
                title={s.name}
                left={<SpaceBadge icon={s.icon} hue={s.hue} theme={s.theme} size={29} />}
                chevron={false}
                onPress={act(() => onMove(tabId, s.id))}
              />
            ))}
          </ListSection>
        )}
      </Animated.View>
    </>
  )
}

const styles = StyleSheet.create({
  sheet: {
    position: 'absolute',
    left: 0,
    right: 0,
    bottom: 0,
    borderTopLeftRadius: radius.card + 4,
    borderTopRightRadius: radius.card + 4,
    paddingTop: space.sm
  },
  grabber: { alignSelf: 'center', width: 36, height: 5, borderRadius: 3, marginBottom: space.md },
  head: { flexDirection: 'row', alignItems: 'center', gap: space.md, paddingHorizontal: space.xl, paddingBottom: space.lg },
  headText: { flex: 1 }
})

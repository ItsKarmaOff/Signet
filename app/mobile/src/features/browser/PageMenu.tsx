// Page menu: sheet rising from the bottom, grouped actions (bookmark, find, reader, text size, share, tab...).
import { useRouter } from 'expo-router'
import { StyleSheet, View, ScrollView, Pressable, useWindowDimensions } from 'react-native'
import Animated, { FadeIn, FadeOut, SlideInDown, SlideOutDown } from 'react-native-reanimated'
import { useSafeAreaInsets } from 'react-native-safe-area-context'
import { siteOf } from '@signet/shared/site'
import type { TabInfo } from '@signet/shared/types'
import { useT } from '@/lib/i18n'
import { Favicon, IconButton, ListRow, ListSection, SwitchRow, Text, gutter, icons, radius, shadow, space, springs, useColors } from '@/ui'
import { MAX_ZOOM, MIN_ZOOM } from './viewStore'

export interface PageMenuActions {
  toggleBookmark: () => void
  find: () => void
  reader: () => void
  zoom: (delta: number) => void
  setDesktop: (on: boolean) => void
  copyLink: () => void
  share: () => void
  openInBrowser: () => void
  togglePin: () => void
  toggleFavorite: () => void
  closeTab: () => void
  fillPassword: () => void
  openOnComputer: () => void
}

interface Props {
  tab: TabInfo
  /** Tab's area within its space. */
  zone: 'favorites' | 'pinned' | 'today' | 'private'
  bookmarked: boolean
  desktop: boolean
  zoom: number
  canFill: boolean
  /** At least one computer connected: "Open on computer". */
  canSendToComputer: boolean
  actions: PageMenuActions
  onClose: () => void
}

export function PageMenu({ tab, zone, bookmarked, desktop, zoom, canFill, canSendToComputer, actions, onClose }: Props) {
  const router = useRouter()
  // Library and settings: the only access from the browser; the menu closes before opening the screen.
  const go = (href: '/library' | '/settings') => {
    onClose()
    router.push(href)
  }
  const t = useT()
  const c = useColors()
  const insets = useSafeAreaInsets()
  const { height } = useWindowDimensions()
  const hasPage = !!tab.url
  const canOrganize = zone !== 'private'
  const title = tab.customTitle || tab.title || siteOf(tab.url) || t('browser.newTabTitle')

  return (
    <>
      <Animated.View entering={FadeIn.duration(160)} exiting={FadeOut.duration(140)} style={[StyleSheet.absoluteFill, { backgroundColor: c.scrim }]}>
        <Pressable style={StyleSheet.absoluteFill} onPress={onClose} accessibilityRole="button" accessibilityLabel={t('common.close')} />
      </Animated.View>
      <Animated.View
        entering={SlideInDown.springify().damping(springs.sheet.damping).stiffness(springs.sheet.stiffness).overshootClamping(1)}
        exiting={SlideOutDown.duration(180)}
        style={[styles.sheet, shadow.pop, { backgroundColor: c.background, bottom: Math.max(insets.bottom, space.sm) + space.xs, maxHeight: height * 0.78 }]}
        accessibilityViewIsModal
      >
        <View style={styles.header}>
          <Favicon uri={tab.favicon} url={tab.url} size={32} />
          <View style={styles.headerText}>
            <Text variant="headline" numberOfLines={1}>
              {title}
            </Text>
            <Text variant="footnote" color="secondary" numberOfLines={1}>
              {siteOf(tab.url) || t('pageMenu.title')}
            </Text>
          </View>
          <IconButton icon={icons.X} label={t('common.close')} onPress={onClose} tone="muted" filled size={18} />
        </View>
        <ScrollView contentContainerStyle={styles.content} showsVerticalScrollIndicator={false}>
          <ListSection>
            <ListRow
              title={bookmarked ? t('pageMenu.removeBookmark') : t('pageMenu.addBookmark')}
              icon={icons.Star}
              iconColor={bookmarked ? c.accent : undefined}
              chevron={false}
              disabled={!hasPage}
              onPress={actions.toggleBookmark}
            />
            {canFill ? <ListRow title={t('passwords.fill')} icon={icons.Key} chevron={false} onPress={actions.fillPassword} /> : null}
            <ListRow title={t('pageMenu.find')} icon={icons.Search} chevron={false} disabled={!hasPage} onPress={actions.find} />
            <ListRow title={t('pageMenu.reader')} icon={icons.BookOpen} chevron={false} disabled={!hasPage} onPress={actions.reader} />
          </ListSection>
          <ListSection>
            <ListRow
              title={t('pageMenu.textSize')}
              icon={icons.TextSize}
              chevron={false}
              disabled={!hasPage}
              right={
                <View style={styles.stepper}>
                  <IconButton
                    icon={icons.Minus}
                    size={18}
                    label={t('pageMenu.smaller')}
                    disabled={!hasPage || zoom <= MIN_ZOOM}
                    onPress={() => actions.zoom(-1)}
                  />
                  <Text variant="subhead" color="secondary" align="center" style={styles.percent}>
                    {zoom}%
                  </Text>
                  <IconButton
                    icon={icons.Plus}
                    size={18}
                    label={t('pageMenu.larger')}
                    disabled={!hasPage || zoom >= MAX_ZOOM}
                    onPress={() => actions.zoom(1)}
                  />
                </View>
              }
            />
            <SwitchRow title={t('pageMenu.desktopSite')} icon={icons.Monitor} value={desktop} onValueChange={actions.setDesktop} disabled={!hasPage} />
          </ListSection>
          <ListSection>
            <ListRow title={t('library.title')} icon={icons.Bookmark} onPress={() => go('/library')} />
            <ListRow title={t('settings.title')} icon={icons.Settings} onPress={() => go('/settings')} />
          </ListSection>
          <ListSection>
            <ListRow title={t('pageMenu.copyLink')} icon={icons.Copy} chevron={false} disabled={!hasPage} onPress={actions.copyLink} />
            <ListRow title={t('pageMenu.share')} icon={icons.Share} chevron={false} disabled={!hasPage} onPress={actions.share} />
            {canSendToComputer ? (
              <ListRow title={t('link.openOnComputer')} icon={icons.Laptop} chevron={false} disabled={!hasPage} onPress={actions.openOnComputer} />
            ) : null}
            <ListRow title={t('pageMenu.openInBrowser')} icon={icons.ExternalLink} chevron={false} disabled={!hasPage} onPress={actions.openInBrowser} />
          </ListSection>
          <ListSection>
            {canOrganize ? (
              <ListRow
                title={zone === 'pinned' ? t('pageMenu.unpin') : t('pageMenu.pin')}
                icon={zone === 'pinned' ? icons.PinOff : icons.Pin}
                chevron={false}
                onPress={actions.togglePin}
              />
            ) : null}
            {canOrganize ? (
              <ListRow
                title={zone === 'favorites' ? t('pageMenu.removeFavorite') : t('pageMenu.addFavorite')}
                icon={icons.Heart}
                chevron={false}
                onPress={actions.toggleFavorite}
              />
            ) : null}
            <ListRow title={t('pageMenu.closeTab')} icon={icons.X} destructive chevron={false} onPress={actions.closeTab} />
          </ListSection>
        </ScrollView>
      </Animated.View>
    </>
  )
}

const styles = StyleSheet.create({
  sheet: { position: 'absolute', left: gutter - 4, right: gutter - 4, borderRadius: radius.card, overflow: 'hidden' },
  header: { flexDirection: 'row', alignItems: 'center', gap: space.md, paddingHorizontal: space.lg, paddingTop: space.lg, paddingBottom: space.md },
  headerText: { flex: 1 },
  content: { paddingTop: space.xs, paddingBottom: space.xs },
  stepper: { flexDirection: 'row', alignItems: 'center' },
  percent: { minWidth: 44 }
})

// Library home: history, bookmarks (with their counts) and a strip of recently visited sites.
import { useMemo } from 'react'
import { Platform, ScrollView, StyleSheet, View } from 'react-native'
import { useRouter } from 'expo-router'
import { bookmarkLinks } from '@signet/shared/mobile/bookmarks'
import { recentSites } from '@signet/shared/mobile/history'
import { siteOf } from '@signet/shared/site'
import { useT } from '@/lib/i18n'
import { useStore } from '@/state'
import { Favicon, ListRow, ListSection, PressableScale, Screen, Text, gutter, icons, radius, space, useColors } from '@/ui'
import { openInBrowser } from './openUrl'

export function LibraryHome() {
  const t = useT()
  const c = useColors()
  const router = useRouter()
  const history = useStore((s) => s.history)
  const bookmarks = useStore((s) => s.bookmarks)
  const downloadCount = useStore((s) => s.downloads.length)
  const linkCount = useMemo(() => bookmarkLinks(bookmarks).length, [bookmarks])
  const recent = useMemo(() => recentSites(history, 10), [history])

  return (
    <Screen scroll contentStyle={{ paddingTop: space.lg, paddingBottom: space.huge }}>
      <ListSection>
        <ListRow
          title={t('library.history')}
          icon={icons.Clock}
          iconColor="#3a82f7"
          value={String(history.length)}
          onPress={() => router.push('/library/history')}
        />
        <ListRow
          title={t('library.bookmarks')}
          icon={icons.Bookmark}
          iconColor="#f59e0b"
          value={String(linkCount)}
          onPress={() => router.push('/library/bookmarks')}
        />
        {Platform.OS === 'ios' ? (
          <ListRow
            title={t('library.downloads')}
            icon={icons.Download}
            iconColor="#10b981"
            value={downloadCount ? String(downloadCount) : undefined}
            onPress={() => router.push('/library/downloads')}
          />
        ) : null}
      </ListSection>

      {recent.length > 0 ? (
        <View>
          <Text variant="footnote" color="secondary" style={styles.heading} accessibilityRole="header">
            {t('library.recent').toUpperCase()}
          </Text>
          <ScrollView horizontal showsHorizontalScrollIndicator={false} contentContainerStyle={styles.strip}>
            {recent.map((e) => {
              const site = siteOf(e.url) || e.url
              return (
                <PressableScale
                  key={e.url}
                  accessibilityRole="link"
                  accessibilityLabel={e.title || site}
                  onPress={() => openInBrowser(router, e.url)}
                  style={[styles.tile, { backgroundColor: c.surface }]}
                >
                  <Favicon uri={e.favicon} url={e.url} size={36} />
                  <Text variant="caption" color="secondary" numberOfLines={1} align="center">
                    {site}
                  </Text>
                </PressableScale>
              )
            })}
          </ScrollView>
        </View>
      ) : null}
    </Screen>
  )
}

const styles = StyleSheet.create({
  heading: { marginLeft: gutter + space.lg, marginBottom: space.sm, letterSpacing: 0.2 },
  strip: { paddingHorizontal: gutter, gap: space.sm },
  tile: { width: 88, paddingVertical: space.md, paddingHorizontal: space.sm, borderRadius: radius.group, alignItems: 'center', gap: space.sm }
})

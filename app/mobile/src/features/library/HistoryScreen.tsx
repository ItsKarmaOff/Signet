// History: search, days as sticky headers, swipe to delete, clearing by range.
import { memo, useCallback, useMemo, useState } from 'react'
import { LayoutAnimation, SectionList, StyleSheet, View } from 'react-native'
import { Stack, useRouter } from 'expo-router'
import { clearRangeStart, countToClear, dayLabel, groupByDay, searchHistory, type ClearRange } from '@signet/shared/mobile/history'
import { siteOf } from '@signet/shared/site'
import type { HistoryEntry } from '@signet/shared/types'
import { haptic } from '@/lib/haptics'
import { useLocale, useT } from '@/lib/i18n'
import { useStore } from '@/state'
import { ActionSheet, Button, EmptyState, Favicon, PressableScale, Screen, SearchField, Text, gutter, icons, space, toast, useColors } from '@/ui'
import { CardRow } from './CardRow'
import { SwipeToDelete } from './SwipeToDelete'
import { openInBrowser } from './openUrl'

interface Section {
  key: string
  title: string
  data: HistoryEntry[]
}

export function HistoryScreen() {
  const t = useT()
  const c = useColors()
  const locale = useLocale()
  const router = useRouter()
  const history = useStore((s) => s.history)
  const removeHistoryEntry = useStore((s) => s.removeHistoryEntry)
  const restoreHistoryEntry = useStore((s) => s.restoreHistoryEntry)
  const clearHistory = useStore((s) => s.clearHistory)
  const [query, setQuery] = useState('')
  const [clearOpen, setClearOpen] = useState(false)
  // The reference time follows history changes: "Today" stays correct without a timer.
  // eslint-disable-next-line react-hooks/exhaustive-deps -- deliberately recomputed on every change of `history`
  const now = useMemo(() => Date.now(), [history])
  const searching = query.trim().length > 0

  const sections = useMemo<Section[]>(() => {
    if (searching) {
      const found = searchHistory(history, query, now, 200)
      return found.length ? [{ key: 'results', title: '', data: found }] : []
    }
    const labels = { today: t('history.today'), yesterday: t('history.yesterday') }
    return groupByDay(history, now).map((d) => ({ key: d.key, title: dayLabel(d, labels, locale, now), data: d.entries }))
    // `t` changes with the language (and so does `locale`)
  }, [history, query, searching, now, locale, t])

  const timeFormat = useMemo(() => new Intl.DateTimeFormat(locale, { hour: 'numeric', minute: '2-digit' }), [locale])

  const open = useCallback((url: string) => openInBrowser(router, url), [router])
  const remove = useCallback(
    (entry: HistoryEntry) => {
      LayoutAnimation.configureNext(LayoutAnimation.Presets.easeInEaseOut)
      removeHistoryEntry(entry.url)
      toast.show({ message: t('history.deleted'), actionLabel: t('common.undo'), onAction: () => restoreHistoryEntry(entry) })
    },
    [removeHistoryEntry, restoreHistoryEntry, t]
  )

  const clear = (range: ClearRange) => {
    const count = countToClear(history, range)
    if (count === 0) {
      toast.show({ message: t('history.nothingToClear') })
      return
    }
    haptic('success')
    clearHistory(clearRangeStart(range))
    toast.show({ message: t('history.cleared', { count }) })
  }

  const ranges: { range: ClearRange; label: string }[] = [
    { range: 'hour', label: t('history.clearHour') },
    { range: 'today', label: t('history.clearToday') },
    { range: 'twoDays', label: t('history.clearTwoDays') },
    { range: 'all', label: t('history.clearAll') }
  ]

  return (
    <Screen>
      <Stack.Screen
        options={{
          headerRight: () => <Button title={t('history.clear')} variant="plain" disabled={history.length === 0} onPress={() => setClearOpen(true)} />
        }}
      />
      {history.length === 0 ? (
        <EmptyState icon={icons.Clock} title={t('library.historyEmpty')} message={t('history.emptyHint')} />
      ) : (
        <>
          <View style={styles.search}>
            <SearchField
              value={query}
              onChangeText={setQuery}
              clearLabel={t('common.clear')}
              placeholder={t('library.searchHistory')}
              accessibilityLabel={t('library.searchHistory')}
            />
          </View>
          {sections.length === 0 ? (
            <EmptyState icon={icons.Search} title={t('history.noResults')} message={t('history.noResultsHint', { query: query.trim() })} />
          ) : (
            <SectionList
              sections={sections}
              keyExtractor={(e) => e.url}
              stickySectionHeadersEnabled
              keyboardShouldPersistTaps="handled"
              keyboardDismissMode="on-drag"
              contentContainerStyle={styles.list}
              renderSectionHeader={({ section }) =>
                section.title ? (
                  <View style={[styles.header, { backgroundColor: c.background }]}>
                    <Text variant="title3" accessibilityRole="header">
                      {section.title}
                    </Text>
                  </View>
                ) : (
                  <View style={styles.headerSpace} />
                )
              }
              renderItem={({ item, index, section }) => (
                <HistoryRow
                  entry={item}
                  time={timeFormat.format(item.lastVisit)}
                  first={index === 0}
                  last={index === section.data.length - 1}
                  deleteLabel={t('history.delete')}
                  onOpen={open}
                  onDelete={remove}
                />
              )}
              ListFooterComponent={<View style={{ height: space.xxxl }} />}
            />
          )}
        </>
      )}
      <ActionSheet
        visible={clearOpen}
        onClose={() => setClearOpen(false)}
        title={t('history.clearTitle')}
        message={t('history.clearMessage')}
        cancelLabel={t('common.cancel')}
        actions={ranges.map((r) => ({ label: r.label, destructive: true, onPress: () => clear(r.range) }))}
      />
    </Screen>
  )
}

interface RowProps {
  entry: HistoryEntry
  time: string
  first: boolean
  last: boolean
  deleteLabel: string
  onOpen: (url: string) => void
  onDelete: (entry: HistoryEntry) => void
}

const HistoryRow = memo(function HistoryRow({ entry, time, first, last, deleteLabel, onOpen, onDelete }: RowProps) {
  const c = useColors()
  const site = siteOf(entry.url) || entry.url
  const title = entry.title || site
  return (
    <CardRow first={first} last={last}>
      <SwipeToDelete label={deleteLabel} onDelete={() => onDelete(entry)}>
        <PressableScale
          accessibilityRole="link"
          accessibilityLabel={title}
          scaleTo={0.99}
          onPress={() => onOpen(entry.url)}
          style={[styles.row, { backgroundColor: c.surface }]}
        >
          <Favicon uri={entry.favicon} url={entry.url} size={30} />
          <View style={styles.texts}>
            <Text variant="body" numberOfLines={1}>
              {title}
            </Text>
            <Text variant="footnote" color="secondary" numberOfLines={1}>
              {site}
            </Text>
          </View>
          <Text variant="footnote" color="tertiary">
            {time}
          </Text>
        </PressableScale>
      </SwipeToDelete>
    </CardRow>
  )
})

const styles = StyleSheet.create({
  search: { paddingHorizontal: gutter, paddingTop: space.sm, paddingBottom: space.md },
  list: { paddingBottom: space.huge },
  header: { paddingHorizontal: gutter + space.xs, paddingTop: space.lg, paddingBottom: space.sm },
  headerSpace: { height: space.xs },
  row: { minHeight: 56, flexDirection: 'row', alignItems: 'center', gap: space.md, paddingHorizontal: space.lg, paddingVertical: space.sm },
  texts: { flex: 1 }
})

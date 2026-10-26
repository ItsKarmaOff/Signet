// Downloads: files fetched from sites, with an icon per type, size and date; tapping opens the preview, swiping deletes.
import { memo, useCallback, useMemo, useState } from 'react'
import { FlatList, LayoutAnimation, StyleSheet, View } from 'react-native'
import { Stack } from 'expo-router'
import { withAlpha } from '@signet/shared/mobile/color'
import { downloadKind, formatBytes, type DownloadEntry, type DownloadKind } from '@signet/shared/mobile/downloads'
import { haptic } from '@/lib/haptics'
import { useLocale, useT } from '@/lib/i18n'
import { useStore } from '@/state'
import { ActionSheet, Button, EmptyState, IconButton, PressableScale, Screen, Text, icons, radius, space, toast, useColors, type LucideIcon } from '@/ui'
import { CardRow } from './CardRow'
import { SwipeToDelete } from './SwipeToDelete'
import { clearAllDownloads, deleteDownload, openDownload, shareDownload } from './downloads'

const KIND_ICON: Record<DownloadKind, LucideIcon> = {
  image: icons.FileImage,
  video: icons.FilePlay,
  audio: icons.FileMusic,
  pdf: icons.FileText,
  archive: icons.FileArchive,
  document: icons.FileText,
  spreadsheet: icons.FileSpreadsheet,
  presentation: icons.Presentation,
  text: icons.FileText,
  code: icons.FileCode,
  app: icons.Package,
  other: icons.File
}

export function DownloadsScreen() {
  const t = useT()
  const locale = useLocale()
  const downloads = useStore((s) => s.downloads)
  const [clearOpen, setClearOpen] = useState(false)
  const decimal = useMemo(() => (new Intl.NumberFormat(locale).format(1.1).includes(',') ? ',' : '.'), [locale])
  const dateFormat = useMemo(() => new Intl.DateTimeFormat(locale, { day: 'numeric', month: 'short', hour: 'numeric', minute: '2-digit' }), [locale])

  const remove = useCallback(
    (entry: DownloadEntry) => {
      LayoutAnimation.configureNext(LayoutAnimation.Presets.easeInEaseOut)
      deleteDownload(entry)
      toast.show({ message: t('downloads.deleted') })
    },
    [t]
  )

  const hasFinished = downloads.some((d) => d.status !== 'downloading')

  return (
    <Screen>
      <Stack.Screen
        options={{ headerRight: () => <Button title={t('downloads.clearAll')} variant="plain" disabled={!hasFinished} onPress={() => setClearOpen(true)} /> }}
      />
      {downloads.length === 0 ? (
        <EmptyState icon={icons.Download} title={t('downloads.empty')} message={t('downloads.emptyHint')} />
      ) : (
        <FlatList
          data={downloads}
          keyExtractor={(d) => d.id}
          contentContainerStyle={styles.list}
          ListHeaderComponent={<View style={{ height: space.lg }} />}
          renderItem={({ item, index }) => (
            <DownloadRow
              entry={item}
              first={index === 0}
              last={index === downloads.length - 1}
              meta={[formatBytes(item.size, decimal), dateFormat.format(item.date)].filter(Boolean).join(' · ')}
              onDelete={remove}
            />
          )}
        />
      )}
      <ActionSheet
        visible={clearOpen}
        onClose={() => setClearOpen(false)}
        title={t('downloads.clearTitle')}
        message={t('downloads.clearMessage')}
        cancelLabel={t('common.cancel')}
        actions={[
          {
            label: t('downloads.clearAll'),
            destructive: true,
            onPress: () => {
              haptic('success')
              clearAllDownloads()
              toast.show({ message: t('downloads.cleared') })
            }
          }
        ]}
      />
    </Screen>
  )
}

interface RowProps {
  entry: DownloadEntry
  first: boolean
  last: boolean
  meta: string
  onDelete: (entry: DownloadEntry) => void
}

const DownloadRow = memo(function DownloadRow({ entry, first, last, meta, onDelete }: RowProps) {
  const c = useColors()
  const t = useT()
  const Icon = KIND_ICON[downloadKind(entry.name)]
  const running = entry.status === 'downloading'
  const failed = entry.status === 'failed'
  const subtitle = running
    ? `${t('downloads.inProgress')}${entry.progress ? ` ${Math.round(entry.progress * 100)}%` : ''}`
    : failed
      ? t('downloads.failedRow')
      : meta
  return (
    <CardRow first={first} last={last}>
      <SwipeToDelete label={t('downloads.delete')} enabled={!running} onDelete={() => onDelete(entry)}>
        <PressableScale
          accessibilityRole="button"
          accessibilityLabel={entry.name}
          disabled={running || failed}
          scaleTo={0.99}
          onPress={() => void openDownload(entry)}
          style={[styles.row, { backgroundColor: c.surface }]}
        >
          <View style={[styles.badge, { backgroundColor: withAlpha(failed ? c.danger : c.accent, 0.14) }]}>
            <Icon size={20} color={failed ? c.danger : c.accent} strokeWidth={2} />
          </View>
          <View style={styles.texts}>
            <Text variant="body" numberOfLines={1}>
              {entry.name}
            </Text>
            <Text variant="footnote" color={failed ? 'danger' : 'secondary'} numberOfLines={1}>
              {subtitle}
            </Text>
          </View>
          {entry.status === 'done' ? (
            <IconButton icon={icons.Share} label={t('downloads.share')} size={18} tone="muted" onPress={() => void shareDownload(entry)} />
          ) : null}
        </PressableScale>
      </SwipeToDelete>
    </CardRow>
  )
})

const styles = StyleSheet.create({
  list: { paddingBottom: space.huge },
  row: { minHeight: 60, flexDirection: 'row', alignItems: 'center', gap: space.md, paddingLeft: space.lg, paddingRight: space.sm, paddingVertical: space.sm },
  badge: { width: 36, height: 36, borderRadius: radius.tile, alignItems: 'center', justifyContent: 'center' },
  texts: { flex: 1 }
})

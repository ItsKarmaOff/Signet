// Clear browsing data: history, tab previews; cookies are explained honestly (see the footer).
import { useEffect, useState } from 'react'
import { LayoutAnimation, View } from 'react-native'
import { haptic } from '@/lib/haptics'
import { useT } from '@/lib/i18n'
import { useStore } from '@/state'
import { ActionSheet, Button, ListRow, ListSection, Screen, gutter, icons, space, toast } from '@/ui'
import { siteDataCleaner, siteDataCount, thumbnailCleaner } from './dataCleaners'

type Kind = 'history' | 'thumbnails' | 'siteData'

export function DataScreen() {
  const t = useT()
  const count = useStore((s) => s.history.length)
  const clearHistory = useStore((s) => s.clearHistory)
  const hasThumbnails = thumbnailCleaner() !== null
  const hasSiteData = siteDataCleaner() !== null
  const available: Kind[] = ['history', ...(hasThumbnails ? (['thumbnails'] as const) : []), ...(hasSiteData ? (['siteData'] as const) : [])]
  const [sites, setSites] = useState<number | null>(null)
  useEffect(() => {
    void siteDataCount().then(setSites)
  }, [])
  const [picked, setPicked] = useState<ReadonlySet<Kind>>(new Set())
  const [confirm, setConfirm] = useState(false)

  const toggle = (kind: Kind) => {
    const next = new Set(picked)
    if (!next.delete(kind)) next.add(kind)
    setPicked(next)
  }
  const allPicked = available.every((k) => picked.has(k))

  const run = async () => {
    LayoutAnimation.configureNext(LayoutAnimation.Presets.easeInEaseOut)
    if (picked.has('history')) clearHistory()
    if (picked.has('thumbnails')) await thumbnailCleaner()?.()
    if (picked.has('siteData')) {
      await siteDataCleaner()?.()
      setSites(await siteDataCount())
    }
    haptic('success')
    setPicked(new Set())
    toast.show({ message: t('data.cleared') })
  }

  return (
    <Screen scroll contentStyle={{ paddingTop: space.lg, paddingBottom: space.huge }}>
      <ListSection title={t('data.intro')} footer={hasSiteData ? undefined : t('data.limits')}>
        <ListRow
          title={t('data.history')}
          subtitle={t('data.historyCount', { count })}
          icon={icons.Clock}
          iconColor="#3a82f7"
          selected={picked.has('history')}
          onPress={() => toggle('history')}
        />
        {hasThumbnails ? (
          <ListRow
            title={t('data.thumbnails')}
            subtitle={t('data.thumbnailsHint')}
            icon={icons.Image}
            iconColor="#34a56f"
            selected={picked.has('thumbnails')}
            onPress={() => toggle('thumbnails')}
          />
        ) : null}
        {hasSiteData ? (
          <ListRow
            title={t('data.siteData')}
            subtitle={sites === null ? t('data.siteDataHint') : `${t('data.siteDataCount', { count: sites })}. ${t('data.siteDataHint')}`}
            icon={icons.Cookie}
            iconColor="#f59e0b"
            selected={picked.has('siteData')}
            onPress={() => toggle('siteData')}
          />
        ) : (
          <ListRow title={t('data.siteData')} subtitle={t('data.siteDataUnavailable')} icon={icons.Cookie} iconColor="#8e8e93" disabled />
        )}
        <ListRow
          title={t('data.all')}
          icon={icons.Eraser}
          iconColor="#ef4444"
          selected={allPicked}
          onPress={() => setPicked(allPicked ? new Set() : new Set(available))}
        />
      </ListSection>
      <View style={{ paddingHorizontal: gutter }}>
        <Button title={t('data.clearButton')} variant="destructive" size="lg" fullWidth disabled={picked.size === 0} onPress={() => setConfirm(true)} />
      </View>
      <ActionSheet
        visible={confirm}
        onClose={() => setConfirm(false)}
        title={t('data.confirmTitle')}
        message={t('data.confirmMessage')}
        cancelLabel={t('common.cancel')}
        actions={[{ label: t('common.clear'), destructive: true, onPress: () => void run() }]}
      />
    </Screen>
  )
}

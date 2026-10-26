import { useState } from 'react'
import { useStore } from '@/state'
import { useT } from '@/lib/i18n'
import { exportBookmarks, importBookmarksFromFile } from '@/features/library'
import { ListRow, ListSection, Screen, icons, space, toast } from '@/ui'

export function TransferScreen() {
  const t = useT()
  const bookmarks = useStore((s) => s.bookmarks)
  const [busy, setBusy] = useState(false)
  const run = async (job: () => Promise<string | null>) => {
    if (busy) return
    setBusy(true)
    try {
      const message = await job()
      if (message) toast.show({ message })
    } finally {
      setBusy(false)
    }
  }
  return (
    <Screen scroll contentStyle={{ paddingTop: space.lg }}>
      <ListSection footer={t('transfer.footer')}>
        <ListRow
          title={t('transfer.exportRow')}
          subtitle={t('transfer.exportHint')}
          icon={icons.Upload}
          iconColor="#3a82f7"
          disabled={busy}
          onPress={() => void run(() => exportBookmarks(bookmarks))}
        />
        <ListRow
          title={t('transfer.importRow')}
          subtitle={t('transfer.importHint')}
          icon={icons.Download}
          iconColor="#34a56f"
          disabled={busy}
          onPress={() => void run(importBookmarksFromFile)}
        />
      </ListSection>
    </Screen>
  )
}

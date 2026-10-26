// "Which computer?" window, mounted once in the root (see `chooseComputer`).
import { useChoice } from './runtime'
import { ActionSheet } from '@/ui'
import { useT } from '@/lib/i18n'

export function LinkHost() {
  const t = useT()
  const choice = useChoice((s) => s.current)
  return (
    <ActionSheet
      visible={!!choice}
      title={choice?.title}
      // The chosen action is handled right after closing: the first answer wins, cancellation comes after.
      onClose={() => {
        const c = choice
        setTimeout(() => c?.settle(null), 0)
      }}
      actions={(choice?.options ?? []).map((o) => ({ label: o.name, onPress: () => choice?.settle(o.linkId) }))}
      cancelLabel={t('common.cancel')}
    />
  )
}

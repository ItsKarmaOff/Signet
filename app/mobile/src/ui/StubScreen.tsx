import { useT } from '@/lib/i18n'
import * as icons from './icons'
import { EmptyState } from './EmptyState'
import { Screen } from './Screen'

/** Placeholder screen for routes not built yet: replace with the real screen from `src/features/<feature>/`. */
export function StubScreen({ title }: { title: string }) {
  const t = useT()
  return (
    <Screen>
      <EmptyState icon={icons.Layers} title={title} message={t('common.comingSoonHint')} />
    </Screen>
  )
}

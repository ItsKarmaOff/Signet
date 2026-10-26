import { StyleSheet, View } from 'react-native'
import { classifyLoadError, type LoadErrorKind } from '@signet/shared/mobile/load-errors'
import { siteOf } from '@signet/shared/site'
import { useT } from '@/lib/i18n'
import { Button, Text, icons, space, useColors, type LucideIcon } from '@/ui'
import type { LoadFailure } from './viewStore'

const ICONS: Record<LoadErrorKind, LucideIcon> = {
  offline: icons.WifiOff,
  notFound: icons.Globe,
  timeout: icons.Clock,
  secure: icons.Lock,
  refused: icons.TriangleAlert,
  other: icons.TriangleAlert
}

const TEXTS: Record<LoadErrorKind, { title: string; message: string }> = {
  offline: { title: 'browser.errorOfflineTitle', message: 'browser.errorOfflineMessage' },
  notFound: { title: 'browser.errorNotFoundTitle', message: 'browser.errorNotFoundMessage' },
  timeout: { title: 'browser.errorTimeoutTitle', message: 'browser.errorTimeoutMessage' },
  secure: { title: 'browser.errorSecureTitle', message: 'browser.errorSecureMessage' },
  refused: { title: 'browser.errorRefusedTitle', message: 'browser.errorRefusedMessage' },
  other: { title: 'browser.errorOtherTitle', message: 'browser.errorOtherMessage' }
}

interface Props {
  failure: LoadFailure
  onRetry: () => void
  /** Height at the bottom of the screen to leave free (toolbar). */
  bottomInset: number
}

/** The app's error page (offline, server not found...): a clear message and a retry button. */
export function ErrorView({ failure, onRetry, bottomInset }: Props) {
  const t = useT()
  const c = useColors()
  const kind = classifyLoadError(failure.code, failure.description)
  const Icon = ICONS[kind]
  const site = siteOf(failure.url)
  return (
    <View style={[styles.fill, { backgroundColor: c.background, paddingBottom: bottomInset }]}>
      <View style={[styles.badge, { backgroundColor: c.surface2 }]}>
        <Icon size={34} color={c.textSecondary} strokeWidth={1.6} />
      </View>
      <Text variant="title3" align="center">
        {t(TEXTS[kind].title)}
      </Text>
      <Text variant="subhead" color="secondary" align="center">
        {t(TEXTS[kind].message)}
      </Text>
      {site ? (
        <Text variant="footnote" color="tertiary" align="center">
          {site}
        </Text>
      ) : null}
      <View style={styles.action}>
        <Button title={t('browser.retry')} icon={icons.RotateCw} onPress={onRetry} />
      </View>
    </View>
  )
}

const styles = StyleSheet.create({
  fill: { ...StyleSheet.absoluteFill, alignItems: 'center', justifyContent: 'center', paddingHorizontal: space.xxxl, gap: space.sm },
  badge: { width: 72, height: 72, borderRadius: 36, alignItems: 'center', justifyContent: 'center', marginBottom: space.md },
  action: { marginTop: space.lg }
})

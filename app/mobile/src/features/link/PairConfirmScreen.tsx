// Pairing confirmation: computer name, then "Pair". Opened by the scanner (Settings) or by the link
// `signet://link?d=…` (the iOS camera reads the QR code and opens the app).
import { useMemo, useState } from 'react'
import { ActivityIndicator, StyleSheet, View } from 'react-native'
import { useLocalSearchParams, useRouter } from 'expo-router'
import { PAIRING_URL_PREFIX, decodePairingUrl } from '@signet/shared/link/protocol'
import { pairErrorKey } from '@signet/shared/mobile/link'
import { useT } from '@/lib/i18n'
import { Button, EmptyState, Screen, Text, gutter, icons, space, useColors } from '@/ui'
import { pairWithOffer } from './manager'
import { useLinkRuntime } from './runtime'

type Phase = { name: 'confirm' } | { name: 'working' } | { name: 'done'; computer: string } | { name: 'error'; key: string }

export function PairConfirmScreen({ sheet = false }: { sheet?: boolean }) {
  const t = useT()
  const c = useColors()
  const router = useRouter()
  const { d } = useLocalSearchParams<{ d?: string }>()
  const native = useLinkRuntime((s) => s.nativeAvailable)
  const [phase, setPhase] = useState<Phase>({ name: 'confirm' })
  const decoded = useMemo(() => (d ? decodePairingUrl(PAIRING_URL_PREFIX + d, Date.now()) : ({ ok: false, error: 'invalid' } as const)), [d])

  const close = () => {
    if (sheet) router.dismissTo('/')
    else router.back()
  }

  const pair = async () => {
    if (!decoded.ok) return
    setPhase({ name: 'working' })
    const r = await pairWithOffer(decoded.offer)
    setPhase(r.ok ? { name: 'done', computer: r.device.name } : { name: 'error', key: pairErrorKey(r.error as never) })
  }

  let body
  if (!decoded.ok) {
    body = <EmptyState icon={icons.TriangleAlert} title={t('link.pairFailed')} message={t(`link.pairError.${pairErrorKey(decoded.error)}`)} />
  } else if (!native) {
    body = <EmptyState icon={icons.TriangleAlert} title={t('link.unavailableTitle')} message={t('link.unavailable')} />
  } else if (phase.name === 'working') {
    body = (
      <View style={styles.center}>
        <ActivityIndicator color={c.accent} />
        <Text variant="title3" align="center">
          {t('link.pairing', { name: decoded.offer.desktop.name })}
        </Text>
      </View>
    )
  } else if (phase.name === 'done') {
    body = <EmptyState icon={icons.Check} title={t('link.pairedTitle', { name: phase.computer })} message={t('link.pairedBody')} />
  } else if (phase.name === 'error') {
    body = <EmptyState icon={icons.TriangleAlert} title={t('link.pairFailed')} message={t(`link.pairError.${phase.key}`)} />
  } else {
    body = <EmptyState icon={icons.Monitor} title={t('link.confirmTitle', { name: decoded.offer.desktop.name })} message={t('link.confirmBody')} />
  }

  const ready = decoded.ok && native
  return (
    <Screen edges={sheet ? ['top', 'bottom'] : ['bottom']}>
      <View style={styles.body}>{body}</View>
      <View style={styles.actions}>
        {ready && phase.name === 'confirm' ? (
          <>
            <Button title={t('link.pair')} size="lg" fullWidth onPress={() => void pair()} />
            <Button title={t('common.cancel')} variant="plain" fullWidth onPress={close} />
          </>
        ) : null}
        {ready && phase.name === 'error' ? (
          <>
            <Button title={t('link.retry')} size="lg" fullWidth onPress={() => void pair()} />
            <Button title={t('common.cancel')} variant="plain" fullWidth onPress={close} />
          </>
        ) : null}
        {(phase.name === 'done' || !ready) && phase.name !== 'working' ? <Button title={t('common.done')} size="lg" fullWidth onPress={close} /> : null}
      </View>
    </Screen>
  )
}

const styles = StyleSheet.create({
  body: { flex: 1, justifyContent: 'center', paddingHorizontal: gutter },
  center: { alignItems: 'center', gap: space.lg },
  actions: { paddingHorizontal: gutter, paddingBottom: space.lg, gap: space.sm }
})

// Dev servers of the connected computers, on the home screen of a new tab: a tap opens the page (via the computer).
import { StyleSheet, View } from 'react-native'
import { serverLabel } from '@signet/shared/mobile/link'
import { useT } from '@/lib/i18n'
import { PressableScale, Text, icons, radius, space, useColors } from '@/ui'
import { useActiveComputers } from './hooks'
import { openDevServerUrl } from './localhost'

export function DevServers({ onOpen }: { onOpen: (url: string) => void }) {
  const t = useT()
  const c = useColors()
  const computers = useActiveComputers().filter((x) => x.servers.length > 0)
  if (computers.length === 0) return null
  return (
    <>
      {computers.map((x) => (
        <View key={x.link.linkId} style={styles.section}>
          <Text variant="footnote" color="secondary" weight="600">
            {t('link.startSection', { name: x.link.desktop.name })}
          </Text>
          <View style={[styles.card, { backgroundColor: c.surface }]}>
            {x.servers.slice(0, 6).map((s, i) => (
              <View key={s.url}>
                {i > 0 ? <View style={[styles.separator, { backgroundColor: c.separator }]} /> : null}
                <PressableScale
                  accessibilityRole="link"
                  accessibilityLabel={serverLabel(s)}
                  haptic="light"
                  scaleTo={0.985}
                  onPress={() => void openDevServerUrl(x.link.linkId, s).then(onOpen)}
                  style={styles.row}
                >
                  <icons.Globe size={18} color={c.accent} strokeWidth={2} />
                  <Text variant="body" numberOfLines={1}>
                    {serverLabel(s)}
                  </Text>
                  {s.title ? (
                    <Text variant="subhead" color="secondary" numberOfLines={1} style={styles.title}>
                      {s.title}
                    </Text>
                  ) : null}
                </PressableScale>
              </View>
            ))}
          </View>
        </View>
      ))}
    </>
  )
}

const styles = StyleSheet.create({
  section: { gap: space.md },
  card: { borderRadius: radius.group, overflow: 'hidden' },
  separator: { height: StyleSheet.hairlineWidth, marginLeft: space.lg },
  row: { minHeight: 48, paddingHorizontal: space.lg, flexDirection: 'row', alignItems: 'center', gap: space.md },
  title: { flexShrink: 1, flexGrow: 1, textAlign: 'right' }
})

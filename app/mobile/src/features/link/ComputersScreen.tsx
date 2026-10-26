// Settings › Computers: master switch, paired computers (state, switch, dev servers, forget), choice for localhost.
import { useState } from 'react'
import { useRouter } from 'expo-router'
import type { LinkStatus } from '@signet/shared/link/status'
import { serverLabel } from '@signet/shared/mobile/link'
import { useT } from '@/lib/i18n'
import { useStore } from '@/state'
import { ActionSheet, ListRow, ListSection, Screen, SwitchRow, Toggle, icons, space, toast, useColors } from '@/ui'
import { openInBrowser } from '@/features/library/openUrl'
import { useComputers, type Computer } from './hooks'
import { chooseLocalhostComputer, forgetLink } from './manager'
import { openDevServerUrl } from './localhost'
import { useLinkRuntime } from './runtime'

const statusKey: Record<LinkStatus, string> = {
  connected: 'connected',
  connecting: 'connecting',
  'paused-here': 'pausedHere',
  'paused-there': 'pausedThere',
  offline: 'offline',
  invalid: 'invalid'
}

export function ComputersScreen() {
  const t = useT()
  const c = useColors()
  const router = useRouter()
  const general = useStore((s) => s.settings.linkEnabled)
  const setSettings = useStore((s) => s.setSettings)
  const patchLink = useStore((s) => s.patchLink)
  const computers = useComputers()
  const native = useLinkRuntime((s) => s.nativeAvailable)
  const supported = useLinkRuntime((s) => s.routeSupported)
  const routeLinkId = useLinkRuntime((s) => s.routeLinkId)
  const chosen = useLinkRuntime((s) => s.chosen)
  const [forgetting, setForgetting] = useState<Computer | null>(null)

  const active = computers.filter((x) => x.status === 'connected')
  const statusText = (x: Computer) => t(`link.status.${statusKey[x.status]}`, { name: x.link.desktop.name })

  const openServer = async (x: Computer, i: number) => {
    const server = x.servers[i]
    if (!server) return
    openInBrowser(router, await openDevServerUrl(x.link.linkId, server))
  }

  return (
    <Screen scroll contentStyle={{ paddingTop: space.lg, paddingBottom: space.huge }}>
      <ListSection footer={native ? t('link.syncFooter') : t('link.unavailable')}>
        <SwitchRow
          title={t('link.sync')}
          icon={icons.Laptop}
          iconColor="#3a82f7"
          value={general}
          onValueChange={(linkEnabled) => setSettings({ linkEnabled })}
        />
        <ListRow title={t('link.pair')} icon={icons.QrCode} iconColor="#34a56f" disabled={!native} onPress={() => router.push('/settings/pair')} />
      </ListSection>

      {computers.map((x) => {
        const invalid = x.status === 'invalid'
        return (
          <ListSection key={x.link.linkId} footer={invalid ? t('link.invalidHint', { name: x.link.desktop.name }) : undefined}>
            {invalid ? (
              <ListRow title={x.link.desktop.name} subtitle={statusText(x)} icon={icons.Monitor} iconColor={c.danger} />
            ) : (
              <ListRow
                title={x.link.desktop.name}
                subtitle={statusText(x)}
                icon={icons.Monitor}
                iconColor={x.status === 'connected' ? '#34a56f' : '#8e8e93'}
                // Master switch off: greyed out, but it keeps its position.
                right={
                  <Toggle
                    value={x.link.enabled}
                    disabled={!general}
                    onValueChange={(enabled) => patchLink(x.link.linkId, { enabled })}
                    accessibilityLabel={x.link.desktop.name}
                  />
                }
              />
            )}
            {x.servers.map((s, i) => (
              <ListRow key={s.url} title={serverLabel(s)} subtitle={s.title} icon={icons.Globe} iconColor="#f59e0b" onPress={() => void openServer(x, i)} />
            ))}
            {invalid ? (
              <ListRow title={t('link.remove')} destructive chevron={false} onPress={() => void forgetLink(x.link.linkId)} />
            ) : (
              <ListRow title={t('link.forget')} destructive chevron={false} onPress={() => setForgetting(x)} />
            )}
          </ListSection>
        )
      })}

      {active.length > 0 ? (
        <ListSection
          title={t('link.localhost')}
          footer={supported === false ? t('link.unsupported') : active.length > 1 ? t('link.localhostChoose') : t('link.localhostOne')}
        >
          {active.map((x) => (
            <ListRow
              key={x.link.linkId}
              title={x.link.desktop.name}
              selected={active.length > 1 ? routeLinkId === x.link.linkId || (!routeLinkId && chosen === x.link.linkId) : undefined}
              icon={active.length > 1 ? undefined : icons.Monitor}
              iconColor="#34a56f"
              onPress={active.length > 1 ? () => void chooseLocalhostComputer(x.link.linkId) : undefined}
              chevron={false}
            />
          ))}
        </ListSection>
      ) : supported === false ? (
        <ListSection footer={t('link.unsupported')}>
          <ListRow title={t('link.localhost')} subtitle={t('link.unsupportedShort')} icon={icons.TriangleAlert} iconColor="#f59e0b" />
        </ListSection>
      ) : null}

      <ActionSheet
        visible={!!forgetting}
        onClose={() => setForgetting(null)}
        title={t('link.forgetTitle', { name: forgetting?.link.desktop.name ?? '' })}
        message={t('link.forgetBody')}
        actions={[
          {
            label: t('link.forget'),
            destructive: true,
            onPress: () => {
              if (forgetting)
                void forgetLink(forgetting.link.linkId).then(() => toast.show({ message: t('link.forgotten', { name: forgetting.link.desktop.name }) }))
            }
          }
        ]}
        cancelLabel={t('common.cancel')}
      />
    </Screen>
  )
}

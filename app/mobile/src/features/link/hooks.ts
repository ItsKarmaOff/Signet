import { useMemo } from 'react'
import { linkStatus, type LinkStatus } from '@signet/shared/link/status'
import type { DevServer } from '@signet/shared/link/protocol'
import type { StoredLink } from '@signet/shared/mobile/link'
import { useStore } from '@/state'
import { EMPTY_RUNTIME, useLinkRuntime } from './runtime'

export interface Computer {
  link: StoredLink
  status: LinkStatus
  servers: DevServer[]
}

/** Paired computers with their displayed state (switch rules: `linkStatus`). */
export function useComputers(): Computer[] {
  const links = useStore((s) => s.links)
  const general = useStore((s) => s.settings.linkEnabled)
  const byLink = useLinkRuntime((s) => s.byLink)
  return useMemo(
    () =>
      links.map((link) => {
        const rt = byLink[link.linkId] ?? EMPTY_RUNTIME
        const status = linkStatus({
          general,
          device: link.enabled,
          connected: rt.connected,
          connecting: rt.connecting,
          peerPaused: link.peerPaused,
          invalid: link.invalid
        })
        return { link, status, servers: status === 'connected' ? rt.servers : [] }
      }),
    [links, general, byLink]
  )
}

/** Computers usable right now (connected, nothing paused). */
export function useActiveComputers(): Computer[] {
  const all = useComputers()
  return useMemo(() => all.filter((c) => c.status === 'connected'), [all])
}

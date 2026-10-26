import { useEffect, useState } from 'react'
import type { LinkState } from '@shared/types'

/** State of the link with phones (Settings › Devices, toolbar indicator), kept up to date by the main process. */
export function useLinkState(): LinkState | null {
  const [state, setState] = useState<LinkState | null>(null)
  useEffect(() => {
    let alive = true
    const signet = window.signet
    signet
      .getLinkState()
      .then((s) => alive && setState(s))
      .catch(() => {})
    const off = signet.onLinkState((s) => setState(s))
    return () => {
      alive = false
      off()
    }
  }, [])
  return state
}

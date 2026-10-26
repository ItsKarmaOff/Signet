import { useEffect } from 'react'
import { startLinkManager } from './manager'

/** Starts the link with the computers (connections, mDNS, localhost relay) for the whole life of the app. */
export function useLinkManager(): void {
  useEffect(() => startLinkManager(), [])
}

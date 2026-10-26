import { useEffect, useRef, useState } from 'react'
import type { OverlayPayload } from '@shared/types'
import { DropZones } from '../tabs/DropZones'
import { Palette } from './Palette'
import { PeekFrame } from './PeekFrame'
import { Popover } from './Popover'
import { SitePanel } from '../windows/SitePanel'
import { PasswordList, PasswordPrompt } from './PasswordPrompt'
import { Capture } from './Capture'
import { WebAppInstall } from '../windows/WebAppInstall'
import { EXIT_MS } from '../lib/useExit'

/** Route #overlay: command bar or Peek bar, on a transparent background. */
export function Overlay() {
  const [payload, setPayload] = useState<OverlayPayload | null>(null)
  // Every opening of the command bar remounts it: fresh state, no leftover input.
  const [gen, setGen] = useState(0)
  // Animated exit: the surface fades out before unmounting; a new opening cancels it.
  const [closing, setClosing] = useState(false)
  const modeRef = useRef<string | null>(null)
  // Displayed payload and exit state, readable from subscriptions (which do not resubscribe).
  const payloadRef = useRef<OverlayPayload | null>(null)
  const closingRef = useRef(false)
  const exitRef = useRef<ReturnType<typeof setTimeout> | null>(null)
  modeRef.current = payload?.mode ?? null
  payloadRef.current = payload
  closingRef.current = closing

  useEffect(() => {
    const signet = window.signet
    const cancelExit = () => {
      if (exitRef.current) clearTimeout(exitRef.current)
      exitRef.current = null
    }
    const offShow = signet.onOverlayShow((p) => {
      // Identical reopening during the exit: cancel the exit without remounting the surface, so the CSS transitions
      // restart from their current value instead of zero.
      const resume = closingRef.current && !!payloadRef.current && JSON.stringify(payloadRef.current) === JSON.stringify(p)
      cancelExit()
      setClosing(false)
      setPayload(p)
      if (resume) return
      if (
        p.mode === 'palette' ||
        p.mode === 'site' ||
        p.mode === 'popover' ||
        p.mode === 'password' ||
        p.mode === 'passwords' ||
        p.mode === 'capture' ||
        p.mode === 'webapp'
      )
        setGen((g) => g + 1)
    })
    const offHide = signet.onOverlayHide((instant) => {
      cancelExit()
      const mode = modeRef.current
      // Click outside the panel: the main process already removed the view, unmount without exit.
      if (instant) {
        setClosing(false)
        return setPayload(null)
      }
      // Drop zones go right away; the main process keeps the view in place for EXIT_MS for the others.
      if (mode !== 'palette' && mode !== 'site' && mode !== 'popover' && mode !== 'password' && mode !== 'passwords' && mode !== 'webapp' && mode !== 'peek')
        return setPayload(null)
      setClosing(true)
      exitRef.current = setTimeout(() => {
        exitRef.current = null
        setClosing(false)
        setPayload(null)
      }, EXIT_MS)
    })
    void signet.overlayReady() // subscribed: the main process sends the pending payload
    return () => {
      cancelExit()
      offShow()
      offHide()
    }
  }, [])

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape' && !e.isComposing) void window.signet.closeOverlay()
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [])

  if (!payload) return null
  if (payload.mode === 'dropzones') return <DropZones payload={payload} />
  if (payload.mode === 'peek') return <PeekFrame payload={payload} closing={closing} />
  if (payload.mode === 'capture') return <Capture key={gen} payload={payload} />
  return (
    // A click during the exit (Escape or action) targets the page behind: remove the view right away.
    <div className={`ov${closing ? ' closing' : ''}`} onMouseDownCapture={closing ? () => void window.signet.closeOverlay(true) : undefined}>
      {payload.mode === 'popover' ? (
        <Popover key={gen} payload={payload} />
      ) : payload.mode === 'site' ? (
        <SitePanel key={gen} payload={payload} />
      ) : payload.mode === 'password' ? (
        <PasswordPrompt key={gen} payload={payload} />
      ) : payload.mode === 'passwords' ? (
        <PasswordList key={gen} payload={payload} />
      ) : payload.mode === 'webapp' ? (
        <WebAppInstall key={gen} payload={payload} />
      ) : (
        <Palette key={gen} payload={payload} />
      )}
    </div>
  )
}

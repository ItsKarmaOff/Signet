import { useEffect, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { X } from 'lucide-react'

/** Display duration before disappearing (pausing on hover or focus extends it). */
const DURATION = 5000
/** Exit duration: the time of the CSS transition before unmounting. */
const EXIT = 240

export interface ToastData {
  /** Changes on every new announcement: restarts the timer. */
  key: number
  message: string
  action?: { label: string; run: () => void }
}

/**
 * Brief announcement with an action (e.g. Undo), placed at the bottom of the sidebar.
 * Entry and exit use the same transition (class `on`); `toast` set to null makes it leave.
 */
export function Toast({ toast, onDismiss, onGone }: { toast: ToastData | null; onDismiss: () => void; onGone?: () => void }) {
  const { t } = useTranslation()
  // Content kept during the exit.
  const [shown, setShown] = useState<ToastData | null>(toast)
  const [on, setOn] = useState(false)
  const [paused, setPaused] = useState(false)
  const dismiss = useRef(onDismiss)
  dismiss.current = onDismiss
  const gone = useRef(onGone)
  gone.current = onGone
  const wasShown = useRef(false)

  useEffect(() => {
    if (toast) {
      setShown(toast)
      wasShown.current = true
      // Two frames: the transition starts from the hidden state.
      let id2 = 0
      const id1 = requestAnimationFrame(() => (id2 = requestAnimationFrame(() => setOn(true))))
      return () => (cancelAnimationFrame(id1), cancelAnimationFrame(id2))
    }
    setOn(false)
    const id = setTimeout(() => {
      setShown(null)
      // Exit finished: the host (floating view) can disappear.
      if (wasShown.current) gone.current?.()
    }, EXIT)
    return () => clearTimeout(id)
  }, [toast])

  // Unmounted under the cursor: no mouseleave, release the pause.
  useEffect(() => setPaused(false), [toast?.key])

  useEffect(() => {
    if (!toast || paused) return
    const id = setTimeout(() => dismiss.current(), DURATION)
    return () => clearTimeout(id)
  }, [toast, paused])

  // Escape closes the announcement without undoing anything (unless it is already used to close a field or a panel).
  useEffect(() => {
    if (!toast) return
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== 'Escape' || e.defaultPrevented) return
      if (e.target instanceof Element && e.target.closest('input, textarea, select, [contenteditable="true"]')) return
      dismiss.current()
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [toast])

  if (!shown) return null
  return (
    <div
      className={`toast${on ? ' on' : ''}`}
      role="status"
      aria-live="polite"
      onMouseEnter={() => setPaused(true)}
      onMouseLeave={() => setPaused(false)}
      onFocus={() => setPaused(true)}
      onBlur={() => setPaused(false)}
    >
      <span className="toast-msg">{shown.message}</span>
      {shown.action && (
        <button
          className="toast-action"
          tabIndex={on ? 0 : -1}
          onClick={() => {
            shown.action?.run()
            dismiss.current()
          }}
        >
          {shown.action.label}
        </button>
      )}
      <button className="icon toast-close" title={t('common.close')} aria-label={t('common.close')} tabIndex={on ? 0 : -1} onClick={() => dismiss.current()}>
        <X size={14} />
      </button>
    </div>
  )
}

/** "… · Undo" announcement (archiving, closed group or folder, deleted space): the main process triggers it, one at a time. */
export function ArchiveToast({ onGone }: { onGone?: () => void } = {}) {
  const { t } = useTranslation()
  const [toast, setToast] = useState<ToastData | null>(null)
  useEffect(
    () =>
      window.signet.onArchived(({ id, kind, count, name }) =>
        setToast({
          key: id,
          message:
            kind === 'spaceDeleted' ? t('sidebar.spaceDeleted', { name }) : t(kind === 'archived' ? 'sidebar.archived' : 'sidebar.tabsClosed', { count }),
          action: { label: t('common.undo'), run: () => void window.signet.undoArchive(id) }
        })
      ),
    [t]
  )
  // Undo window elapsed on the main side: no more Undo to offer.
  useEffect(() => window.signet.onUndoExpired((id) => setToast((cur) => (cur?.key === id ? null : cur))), [])
  return <Toast toast={toast} onDismiss={() => setToast(null)} onGone={onGone} />
}

/** Route #toast: the same announcement in its own small view (tabs on top, compact, sidebar hidden). */
export function ToastWindow() {
  // Subscription done (the child's effect runs before this one): the main process replays the pending announcement.
  useEffect(() => void window.signet.overlayReady(), [])
  return <ArchiveToast onGone={() => void window.signet.toastDone()} />
}

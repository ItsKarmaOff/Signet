import { useEffect, useRef } from 'react'
import { useTranslation } from 'react-i18next'

/**
 * In-place confirmation of a destructive action: a message naming the object and the consequence,
 * then Cancel (default focus) and the action button in `danger` style. Escape cancels.
 */
export function InlineConfirm({
  message,
  confirmLabel,
  onConfirm,
  onCancel,
  btnClass = 'st-btn',
  messageClass = 'st-hint'
}: {
  message: string
  confirmLabel: string
  onConfirm: () => void
  onCancel: () => void
  btnClass?: string
  messageClass?: string
}) {
  const { t } = useTranslation()
  const cancel = useRef<HTMLButtonElement>(null)
  useEffect(() => cancel.current?.focus(), [])
  return (
    <span
      className="inline-confirm"
      role="alertdialog"
      aria-label={message}
      style={{ display: 'contents' }}
      onKeyDown={(e) => {
        if (e.key !== 'Escape') return
        e.preventDefault()
        e.stopPropagation()
        onCancel()
      }}
    >
      <span className={messageClass}>{message}</span>
      <button ref={cancel} className={btnClass} onClick={onCancel}>
        {t('common.cancel')}
      </button>
      <button className={`${btnClass} danger solid`} onClick={onConfirm}>
        {confirmLabel}
      </button>
    </span>
  )
}

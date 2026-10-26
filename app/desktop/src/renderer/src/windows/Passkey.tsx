import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import type { PasskeyPromptState } from '@shared/types'

/** Route #passkey: QR code to scan with a phone (passkey from another device). Escape or Cancel gives up. */
export function Passkey() {
  const { t } = useTranslation()
  const [prompt, setPrompt] = useState<PasskeyPromptState | null>(null)
  const cancel = () => void window.signet.cancelPasskeyPrompt().catch(() => {})

  useEffect(() => {
    // Subscribe first: a status pushed while the prompt is fetched is not lost.
    const off = window.signet.onPasskeyStatus((status) => setPrompt((p) => p && { ...p, status }))
    void window.signet
      .getPasskeyPrompt()
      .then(setPrompt)
      .catch(() => {})
    return off
  }, [])
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape' && !e.isComposing) void window.signet.cancelPasskeyPrompt().catch(() => {})
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [])

  const kind = prompt?.type ?? 'get'
  const status = prompt?.status ?? 'waiting'
  // Waiting (and no scan available): the plain hint; otherwise what is going on.
  const plain = status === 'waiting' || status === 'unsupported'
  const hint = plain ? 'passkey.hint' : (`passkey.status.${status}` as 'passkey.hint')
  return (
    <div className="welcome passkey">
      <div className="wl-drag" />
      <main className="wl-main">
        <h1>{t(`passkey.${kind}Title`)}</h1>
        <p className="wl-text">{prompt && t(`passkey.${kind}Text`, { site: prompt.rpId })}</p>
        <div className="dv-qr">{prompt && <img src={prompt.qr} alt={t('passkey.qrAlt')} draggable={false} />}</div>
        <p className={`st-hint pk-hint${plain ? '' : status === 'connecting' || status === 'confirm' ? ' pk-found' : ' pk-warn'}`} role="status">
          {t(hint)}
        </p>
        <button className="st-btn pk-cancel" onClick={cancel} autoFocus>
          {t('passkey.cancel')}
        </button>
      </main>
    </div>
  )
}

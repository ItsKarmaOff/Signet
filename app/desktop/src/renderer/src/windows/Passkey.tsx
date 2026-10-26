import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import type { PasskeyPromptState } from '@shared/types'

/** Route #passkey: QR code to scan with a phone (passkey from another device). Escape or Cancel gives up. */
export function Passkey() {
  const { t } = useTranslation()
  const [prompt, setPrompt] = useState<PasskeyPromptState | null>(null)
  const cancel = () => void window.signet.cancelPasskeyPrompt().catch(() => {})

  useEffect(() => {
    void window.signet
      .getPasskeyPrompt()
      .then(setPrompt)
      .catch(() => {})
  }, [])
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape' && !e.isComposing) void window.signet.cancelPasskeyPrompt().catch(() => {})
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [])

  const kind = prompt?.type ?? 'get'
  return (
    <div className="welcome passkey">
      <div className="wl-drag" />
      <main className="wl-main">
        <h1>{t(`passkey.${kind}Title`)}</h1>
        <p className="wl-text">{prompt && t(`passkey.${kind}Text`, { site: prompt.rpId })}</p>
        <div className="dv-qr">{prompt && <img src={prompt.qr} alt={t('passkey.qrAlt')} draggable={false} />}</div>
        <p className="st-hint pk-hint">{t('passkey.hint')}</p>
        <button className="st-btn pk-cancel" onClick={cancel} autoFocus>
          {t('passkey.cancel')}
        </button>
      </main>
    </div>
  )
}

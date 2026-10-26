import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { Check, Copy, KeyRound, Settings as Cog } from 'lucide-react'
import type { OverlayPayload, Rect } from '@shared/types'

type Prompt = Extract<OverlayPayload, { mode: 'password' }>
type List = Extract<OverlayPayload, { mode: 'passwords' }>

const WIDTH = 300

/** Card placed under the key button: right edge aligned with it, arrow pointing at its center. */
function underAnchor(anchor: Rect) {
  const left = Math.max(8, Math.min(anchor.x + anchor.width + 6 - WIDTH, window.innerWidth - WIDTH - 8))
  const arrow = Math.max(14, Math.min(anchor.x + anchor.width / 2 - left, WIDTH - 14))
  return { left, top: anchor.y + anchor.height + 8, arrow }
}

/** Shadow margin around the card when the overlay covers only it (= PW_MARGIN in main/browser/overlay.ts). */
const MARGIN = 24

/**
 * Offer to save a password (route #overlay, mode `password`).
 * Opened from the key button: under it, full screen, a click outside = later.
 * Unsolicited (after a login): the overlay covers only the card, with no focus or autoFocus, so the page
 * stays clickable and keeps the keyboard (Enter while typing a code must not save it); the main process puts it away after a while.
 */
export function PasswordPrompt({ payload }: { payload: Prompt }) {
  const { t } = useTranslation()
  const { token, origin, username, update, anchor } = payload
  const answer = (a: 'save' | 'never' | 'dismiss') => void window.signet.answerPasswordPrompt(token, a)
  // Opened from the key button: under it; after a login: top right of the page.
  const placed = anchor ? underAnchor(anchor) : null
  const compact = !anchor
  const left = placed?.left ?? MARGIN
  const top = placed?.top ?? MARGIN
  const host = origin.replace(/^\w+:\/\//, '')
  return (
    <div
      className="site-layer"
      onMouseDown={
        compact
          ? undefined
          : () => {
              answer('dismiss')
              void window.signet.closeOverlay()
            }
      }
    >
      <div
        className="site-panel pw-prompt"
        style={{
          left,
          top,
          width: WIDTH,
          transformOrigin: placed ? `${placed.arrow}px -8px` : 'top right',
          ...(placed && { ['--arrow' as string]: `${placed.arrow}px` })
        }}
        onMouseDown={(e) => e.stopPropagation()}
      >
        {placed && <i className="site-arrow" />}
        <div className="site-head">
          <KeyRound size={18} className="site-ico" />
          <div className="site-id">
            <strong>{update ? t('password.updateTitle') : t('password.saveTitle')}</strong>
            <span className="site-conn">{host}</span>
          </div>
        </div>
        <div className="pw-user">{username || t('password.noUsername')}</div>
        <div className="site-foot pw-actions">
          <button className="site-btn-s" onClick={() => answer('never')}>
            {t('password.never')}
          </button>
          <span className="pw-grow" />
          <button className="site-btn-s" onClick={() => answer('dismiss')}>
            {t('password.notNow')}
          </button>
          <button className="site-btn-s pw-primary" autoFocus={!compact} onClick={() => answer('save')}>
            {update ? t('password.update') : t('password.save')}
          </button>
        </div>
      </div>
    </div>
  )
}

/** Accounts saved for the site (route #overlay, `passwords` mode), under the key button; never a password here. */
export function PasswordList({ payload }: { payload: List }) {
  const { t } = useTranslation()
  const { origin, logins, anchor } = payload
  const [copied, setCopied] = useState<string | null>(null)
  const { left, top, arrow } = underAnchor(anchor)
  const host = origin.replace(/^\w+:\/\//, '')
  const copy = (id: string, username: string) => {
    void navigator.clipboard
      .writeText(username)
      .then(() => setCopied(id))
      .catch(() => {})
  }
  return (
    <div className="site-layer" onMouseDown={() => void window.signet.closeOverlay()}>
      <div
        className="site-panel pw-prompt"
        style={{ left, top, width: WIDTH, transformOrigin: `${arrow}px -8px`, ['--arrow' as string]: `${arrow}px` }}
        onMouseDown={(e) => e.stopPropagation()}
      >
        <i className="site-arrow" />
        <div className="site-head">
          <KeyRound size={18} className="site-ico" />
          <div className="site-id">
            <strong>{t('password.listTitle')}</strong>
            <span className="site-conn">{host}</span>
          </div>
        </div>
        <div className="pw-list">
          {logins.map((l) => (
            <div key={l.id} className="pw-login">
              <span className="pw-login-name">{l.username || t('password.noUsername')}</span>
              {l.username && (
                <button
                  className="site-btn-s"
                  title={t('password.copyUsername')}
                  aria-label={t('password.copyUsername')}
                  onClick={() => copy(l.id, l.username)}
                >
                  {copied === l.id ? <Check size={13} strokeWidth={2.4} /> : <Copy size={13} />}
                  {copied === l.id ? t('password.copied') : t('password.copyUsernameShort')}
                </button>
              )}
            </div>
          ))}
        </div>
        <div className="site-foot pw-actions">
          <button
            className="site-btn-s"
            onClick={() => {
              void window.signet.openSettings('passwords')
              void window.signet.closeOverlay()
            }}
          >
            <Cog size={13} />
            {t('password.manage')}
          </button>
        </div>
      </div>
    </div>
  )
}

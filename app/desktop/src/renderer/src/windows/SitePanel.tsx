import { useEffect, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { Camera, ClipboardPaste, Bell, MapPin, Lock, Info, Rocket, ShieldOff, Trash2 } from 'lucide-react'
import type { OverlayPayload, PermissionState } from '@shared/types'
import { Favicon } from '../ui/Favicon'

type Site = Extract<OverlayPayload, { mode: 'site' }>

const WIDTH = 320
const STATES: PermissionState[] = ['ask', 'allow', 'deny']
const PERM_KEYS = {
  media: 'site.perm.media',
  geolocation: 'site.perm.geolocation',
  notifications: 'site.perm.notifications',
  'clipboard-read': 'site.perm.clipboard-read'
} as const
const ICONS: Record<string, typeof Camera> = { media: Camera, geolocation: MapPin, notifications: Bell, 'clipboard-read': ClipboardPaste }

/** Site panel: card under the padlock (route #overlay, `site` mode), no scrim; a click outside closes it. */
export function SitePanel({ payload }: { payload: Site }) {
  const { t } = useTranslation()
  const signet = window.signet
  const { info, anchor } = payload
  const [perms, setPerms] = useState(info.permissions)
  const [boost, setBoost] = useState(info.boost)
  const [confirm, setConfirm] = useState(false)
  const [cleared, setCleared] = useState(false)
  const [failed, setFailed] = useState(false)
  const closeTimer = useRef<number>(undefined)
  useEffect(() => () => window.clearTimeout(closeTimer.current), [])
  const cx = anchor.x + anchor.width / 2
  const left = Math.max(8, Math.min(anchor.x - 6, window.innerWidth - WIDTH - 8))
  const top = anchor.y + anchor.height + 8
  const arrow = Math.max(14, Math.min(cx - left, WIDTH - 14))

  const setPerm = (id: string, state: PermissionState) => {
    setPerms((p) => p.map((x) => (x.id === id ? { ...x, state } : x)))
    void signet.setSitePermission(info.origin, id, state)
  }
  const host = info.origin.replace(/^\w+:\/\//, '')

  return (
    <div className="site-layer" onMouseDown={() => signet.closeOverlay()}>
      <div
        className="site-panel"
        style={{ left, top, width: WIDTH, transformOrigin: `${arrow}px -8px`, ['--arrow' as string]: `${arrow}px` }}
        onMouseDown={(e) => e.stopPropagation()}
      >
        <i className="site-arrow" />
        <div className="site-head">
          <Favicon size={20} src={info.favicon} />
          <div className="site-id">
            <strong>{host}</strong>
            <span className={`site-conn${info.secure ? '' : ' warn'}`}>
              {info.secure ? <Lock size={11} /> : <Info size={11} />}
              {info.secure ? t('site.secure') : t('site.insecure')}
            </span>
          </div>
        </div>

        <div className="site-section">{t('site.permissions')}</div>
        <div className="site-perms">
          {perms.map((p) => {
            const Icon = ICONS[p.id] ?? Info
            const label = p.id in PERM_KEYS ? t(PERM_KEYS[p.id as keyof typeof PERM_KEYS]) : p.label
            return (
              <div className="site-perm" key={p.id}>
                <Icon size={15} className="site-ico" />
                <span className="site-label">{label}</span>
                <div className="seg-ctl" role="radiogroup" aria-label={label}>
                  {STATES.map((st) => (
                    <button key={st} role="radio" aria-checked={p.state === st} className={p.state === st ? `on ${st}` : ''} onClick={() => setPerm(p.id, st)}>
                      {t(`site.state.${st}`)}
                    </button>
                  ))}
                </div>
              </div>
            )
          })}
        </div>

        <div className="site-row">
          <ShieldOff size={15} className="site-ico" />
          <span className="site-label">{info.blocked === 0 ? t('site.blockedNone') : t('site.blocked', { count: info.blocked })}</span>
        </div>

        <div className="site-row">
          <Rocket size={15} className="site-ico" />
          <span className="site-label">{boost.exists ? t('site.boost') : t('site.noBoost')}</span>
          {boost.exists && (
            <button
              className={`switch ${boost.enabled ? 'on' : ''}`}
              role="switch"
              aria-checked={boost.enabled}
              aria-label={t('site.boostActive')}
              onClick={() => {
                setBoost({ exists: true, enabled: !boost.enabled })
                void signet.saveBoost(info.site, { enabled: !boost.enabled })
              }}
            >
              <span className="knob" />
            </button>
          )}
          <button
            className="site-btn-s"
            onClick={() => {
              void signet.openBoost(info.site)
              void signet.closeOverlay()
            }}
          >
            {boost.exists ? t('site.edit') : t('site.create')}
          </button>
        </div>

        <div className="site-foot">
          {confirm ? (
            <>
              {failed ? (
                <span className="site-confirm site-err" role="alert">
                  {t('site.clearFailed')}
                </span>
              ) : (
                <span className="site-confirm">{t('site.confirmClear')}</span>
              )}
              <button
                className="site-btn-s"
                onClick={() => {
                  setConfirm(false)
                  setFailed(false)
                }}
              >
                {t('common.cancel')}
              </button>
              <button
                className="site-btn-s danger"
                onClick={async () => {
                  try {
                    await signet.clearSiteData(info.origin)
                  } catch {
                    setFailed(true)
                    return
                  }
                  setFailed(false)
                  setConfirm(false)
                  setCleared(true)
                  closeTimer.current = window.setTimeout(() => signet.closeOverlay(), 700)
                }}
              >
                {t('site.clear')}
              </button>
            </>
          ) : (
            <button className="site-clear" disabled={cleared} onClick={() => setConfirm(true)}>
              <Trash2 size={14} />
              {cleared ? t('site.cleared') : t('site.clearData')}
            </button>
          )}
        </div>
      </div>
    </div>
  )
}

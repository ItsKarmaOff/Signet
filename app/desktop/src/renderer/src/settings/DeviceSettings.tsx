import { useEffect, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { RefreshCw, Smartphone, Trash2 } from 'lucide-react'
import type { LinkPlatform } from '@signet/shared/link/protocol'
import type { LinkDeviceInfo } from '@shared/types'
import { InlineConfirm } from '../ui/InlineConfirm'
import { useLinkState } from '../lib/useLinkState'

const PLATFORMS: Record<LinkPlatform, string> = { ios: 'iOS', android: 'Android', macos: 'macOS', windows: 'Windows', linux: 'Linux' }

const clock = (ms: number): string => {
  const s = Math.max(0, Math.ceil(ms / 1000))
  return `${Math.floor(s / 60)}:${String(s % 60).padStart(2, '0')}`
}

function Switch({ on, label, disabled, onChange }: { on: boolean; label: string; disabled?: boolean; onChange: (on: boolean) => void }) {
  return (
    <button className={`switch ${on ? 'on' : ''}`} role="switch" aria-checked={on} aria-label={label} disabled={disabled} onClick={() => onChange(!on)}>
      <span className="knob" />
    </button>
  )
}

/** Settings › Devices: master switch, paired phones, pairing by QR code. */
export function DeviceSettings() {
  const { t } = useTranslation()
  const signet = window.signet
  const link = useLinkState()
  const [confirming, setConfirming] = useState<string | null>(null)
  /** QR code area open (current, expired or failed code). */
  const [pairing, setPairing] = useState(false)
  const [starting, setStarting] = useState(false)
  const [failed, setFailed] = useState(false)
  const [notice, setNotice] = useState<string | null>(null)
  const [now, setNow] = useState(() => Date.now())
  /** Links known when pairing started: a new link = the phone is paired. */
  const before = useRef<Set<string>>(new Set())
  const open = useRef(false)
  open.current = pairing

  // Leaving the section (or the window) cancels the current code.
  useEffect(
    () => () => {
      if (open.current) void signet.cancelLinkPairing()
    },
    [signet]
  )
  const expiresAt = link?.pairing?.expiresAt
  useEffect(() => {
    if (!expiresAt) return
    setNow(Date.now())
    const timer = setInterval(() => setNow(Date.now()), 500)
    return () => clearInterval(timer)
  }, [expiresAt])

  const added = link?.devices.find((d) => !before.current.has(d.linkId))
  // The phone paired: the area closes and the message names it.
  useEffect(() => {
    if (!pairing || starting || link?.pairing || !added) return
    setPairing(false)
    setNotice(t('settings.devices.paired', { name: added.device.name }))
  }, [pairing, starting, link?.pairing, added, t])

  if (!link) return null
  const { devices, general } = link

  const start = async () => {
    before.current = new Set(devices.map((d) => d.linkId))
    setNotice(null)
    setFailed(false)
    setPairing(true)
    setStarting(true)
    try {
      setFailed(!(await signet.startLinkPairing()))
    } catch {
      setFailed(true)
    }
    setStarting(false)
  }
  const cancel = () => {
    setPairing(false)
    void signet.cancelLinkPairing()
  }
  const remaining = link.pairing ? link.pairing.expiresAt - now : 0
  const expired = pairing && !starting && !failed && !added && (!link.pairing || remaining <= 0)

  const statusText = (d: LinkDeviceInfo) => `${PLATFORMS[d.device.platform]} · ${t(`settings.devices.status.${d.status}`, { name: d.device.name })}`

  return (
    <>
      <div className="st-group">
        <div className="st-row">
          <div className="st-label">
            <span>{t('settings.devices.general')}</span>
            <span className="st-hint">{t('settings.devices.generalHint')}</span>
          </div>
          <Switch on={general} label={t('settings.devices.general')} onChange={(on) => void signet.setLinkGeneral(on)} />
        </div>
      </div>

      <h3 className="st-sub">{t('settings.devices.phones')}</h3>
      <div className="st-group">
        {devices.map((d) => (
          <div key={d.linkId} className={`st-row dv-row${general ? '' : ' dv-off'}`}>
            <Smartphone size={22} strokeWidth={1.5} className="dv-icon" />
            <div className="st-label">
              <span>{d.device.name}</span>
              <span className="st-hint">{statusText(d)}</span>
            </div>
            {confirming === d.linkId ? (
              <InlineConfirm
                message={t('settings.devices.forgetConfirm', { name: d.device.name })}
                confirmLabel={t('settings.devices.forget')}
                onCancel={() => setConfirming(null)}
                onConfirm={() => {
                  setConfirming(null)
                  void signet.forgetLink(d.linkId)
                }}
              />
            ) : (
              <>
                <button className="icon" title={t('settings.devices.forget')} aria-label={t('settings.devices.forget')} onClick={() => setConfirming(d.linkId)}>
                  <Trash2 size={15} />
                </button>
                <Switch
                  on={d.enabled}
                  disabled={!general}
                  label={t('settings.devices.toggle', { name: d.device.name })}
                  onChange={(on) => void signet.setLinkDevice(d.linkId, on)}
                />
              </>
            )}
          </div>
        ))}
        <div className="st-row">
          {devices.length === 0 && (
            <div className="st-label">
              <span>{t('settings.devices.empty')}</span>
              <span className="st-hint">{t('settings.devices.emptyHint')}</span>
            </div>
          )}
          {devices.length > 0 && !general && <span className="st-hint grow">{t('settings.devices.generalOffNote')}</span>}
          <button className="st-btn" disabled={!general || pairing} onClick={() => void start()}>
            <Smartphone size={13} />
            {t('settings.devices.pair')}
          </button>
        </div>
      </div>
      {notice && !pairing && <p className="st-note st-ok">{notice}</p>}

      {pairing && (
        <div className="st-group dv-pair">
          <div className="st-row">
            <div className="dv-qr" aria-busy={starting}>
              {link.pairing && !expired ? (
                <img src={link.pairing.qr} alt={t('settings.devices.pairQrLabel')} draggable={false} />
              ) : (
                <div className="dv-qr-empty" />
              )}
            </div>
            <div className="st-label">
              <span>{t('settings.devices.pairHow')}</span>
              {failed ? (
                <span className="st-err" role="alert">
                  {t('settings.devices.pairFailed')}
                </span>
              ) : expired ? (
                <span className="st-hint">{t('settings.devices.pairExpired')}</span>
              ) : link.pairing ? (
                <span className="st-hint">{t('settings.devices.pairExpires', { time: clock(remaining) })}</span>
              ) : null}
            </div>
          </div>
          <div className="st-row st-actions">
            <button className="st-btn" onClick={cancel}>
              {t('common.cancel')}
            </button>
            <button className="st-btn" disabled={starting} onClick={() => void start()}>
              <RefreshCw size={13} />
              {t('settings.devices.pairNew')}
            </button>
          </div>
        </div>
      )}
    </>
  )
}

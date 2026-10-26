import { useEffect, useState } from 'react'
import { ArrowLeft, Check, Copy, ExternalLink, House, Lock, RotateCw, X } from 'lucide-react'
import { useTranslation } from 'react-i18next'
import type { WebAppState } from '@shared/types'
import { barStyle, domainOf, useHue } from '../lib/util'

/**
 * Route #webapp: title bar of an installed app, above its page. Site color, page title;
 * when the page leaves the app's site (login, external link), its domain and a button to go back.
 */
export function WebApp() {
  const { t } = useTranslation()
  const [st, setSt] = useState<WebAppState | null>(null)
  const [copied, setCopied] = useState(false)
  useHue(st?.hue)

  useEffect(() => {
    const off = window.signet.onWebAppState(setSt)
    void window.signet.webAppReady() // subscribed: main sends back the current state
    return off
  }, [])
  useEffect(() => {
    if (!copied) return
    const timer = setTimeout(() => setCopied(false), 1400)
    return () => clearTimeout(timer)
  }, [copied])

  const signet = window.signet
  const away = st ? !st.inScope : false
  return (
    <div className="wa-bar" style={barStyle(st?.themeColor)}>
      <div className="wa-nav">
        <button className="icon" title={t('common.back')} disabled={!st?.canGoBack} onClick={() => signet.goBack()}>
          <ArrowLeft size={16} />
        </button>
        <button className="icon" title={st?.loading ? t('toolbar.stop') : t('toolbar.reload')} onClick={() => signet.reload()}>
          {st?.loading ? <X size={16} /> : <RotateCw size={15} />}
        </button>
      </div>
      <div className="wa-title">
        {away && st ? (
          <>
            <span className={`wa-origin${/^https:/i.test(st.url) ? '' : ' insecure'}`}>
              {/^https:/i.test(st.url) && <Lock size={11} strokeWidth={2.4} />}
              {domainOf(st.url)}
            </span>
            {st.title && <span className="t dim">{st.title}</span>}
          </>
        ) : (
          <>
            {st?.icon && <img className="wa-icon" src={st.icon} alt="" draggable={false} />}
            <span className="t">{st?.title || st?.name || ''}</span>
          </>
        )}
      </div>
      <div className="wa-actions">
        {away && (
          <button className="icon" title={t('webapp.home', { name: st?.name ?? '' })} onClick={() => signet.webAppHome()}>
            <House size={15} />
          </button>
        )}
        <button
          className="icon"
          title={t('little.copyUrl')}
          onClick={() => {
            void signet.webAppCopyUrl()
            setCopied(true)
          }}
        >
          {copied ? <Check size={15} /> : <Copy size={15} />}
        </button>
        <button className="icon" title={t('little.openInMain')} onClick={() => signet.webAppOpenInMain()}>
          <ExternalLink size={15} />
        </button>
      </div>
    </div>
  )
}

import { useEffect, useState } from 'react'
import { ArrowLeft, ArrowRight, Check, Copy, ExternalLink } from 'lucide-react'
import type { LittleState } from '@shared/types'
import { useTranslation } from 'react-i18next'
import { Favicon } from '../ui/Favicon'
import { barStyle, domainOf, useHue } from '../lib/util'

/** Route #little: 44 px bar above the little window's web view. */
export function Little() {
  const { t } = useTranslation()
  const [st, setSt] = useState<LittleState | null>(null)
  const [copied, setCopied] = useState(false)
  useHue(st?.hue)

  useEffect(() => {
    const off = window.signet.onLittleState(setSt)
    void window.signet.littleReady() // subscribed: main sends back the current state
    return off
  }, [])
  useEffect(() => {
    if (!copied) return
    const t = setTimeout(() => setCopied(false), 1400)
    return () => clearTimeout(t)
  }, [copied])

  const signet = window.signet
  return (
    <>
      <div className="little-strip" />
      <div className="pane-shadow little-card" style={barStyle(st?.themeColor)}>
        <div className="little-fill" />
        <div className="little-bar">
          <button className="icon" title={t('common.back')} disabled={!st?.canGoBack} onClick={() => signet.goBack()}>
            <ArrowLeft size={17} />
          </button>
          <button className="icon" title={t('common.forward')} disabled={!st?.canGoForward} onClick={() => signet.goForward()}>
            <ArrowRight size={17} />
          </button>
          <div className="little-title">
            <Favicon src={st?.favicon} loading={st?.loading} />
            <span className="t">{st?.title || (st?.url ? domainOf(st.url) : '')}</span>
            {st?.url && <span className="d">{domainOf(st.url)}</span>}
          </div>
          <button
            className="icon"
            title={t('little.copyUrl')}
            onClick={() => {
              void signet.littleCopyUrl()
              setCopied(true)
            }}
          >
            {copied ? <Check size={16} /> : <Copy size={16} />}
          </button>
          <button className="icon" title={t('little.openInMain')} onClick={() => signet.littleOpenInMain()}>
            <ExternalLink size={16} />
          </button>
        </div>
      </div>
    </>
  )
}

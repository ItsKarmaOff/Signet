import { useCallback, useEffect, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { Check, Crosshair, Trash2, X } from 'lucide-react'
import { InlineConfirm } from '../ui/InlineConfirm'
import type { BoostEditorState } from '@shared/types'
import { useHue } from '../lib/util'

const PRESETS = [
  {
    id: 'dark',
    label: 'boost.preset.dark',
    css: 'html { filter: invert(1) hue-rotate(180deg); }\nimg, video, picture, canvas, iframe, svg image { filter: invert(1) hue-rotate(180deg); }'
  },
  { id: 'font', label: 'boost.preset.font', css: '* { font-family: -apple-system, system-ui, sans-serif !important; }' },
  { id: 'width', label: 'boost.preset.width', css: 'body { max-width: 860px !important; margin-inline: auto !important; }' },
  {
    id: 'still',
    label: 'boost.preset.still',
    css: '*, *::before, *::after { animation: none !important; transition: none !important; }'
  }
] as const

const open = (id: string) => `/* signet:${id} */`
const close = (id: string) => `/* /signet:${id} */`
const hasBlock = (css: string, id: string) => css.includes(open(id))

function withBlock(css: string, id: string, body: string): string {
  const block = `${open(id)}\n${body}\n${close(id)}`
  return css.trim() ? `${css.replace(/\s+$/, '')}\n\n${block}\n` : `${block}\n`
}

function withoutBlock(css: string, id: string): string {
  const start = css.indexOf(open(id))
  if (start < 0) return css
  const endTag = css.indexOf(close(id), start)
  const end = endTag < 0 ? css.length : endTag + close(id).length
  return (css.slice(0, start).replace(/\s+$/, '') + '\n\n' + css.slice(end).replace(/^\s+/, '')).replace(/^\s+/, '').replace(/\n{3,}/g, '\n\n')
}

/** Route #boost: editing window for a site's Boost. */
export function Boost() {
  const { t } = useTranslation()
  const [st, setSt] = useState<BoostEditorState | null>(null)
  const [css, setCss] = useState('')
  const [confirming, setConfirming] = useState(false)
  const [saved, setSaved] = useState(false)
  const area = useRef<HTMLTextAreaElement>(null)
  const timer = useRef<ReturnType<typeof setTimeout> | null>(null)
  const latest = useRef({ css: '', site: '' })
  const stateRef = useRef<BoostEditorState | null>(null)
  useHue(st?.hue)

  const site = st?.site ?? ''
  latest.current.site = site

  const flush = useCallback(() => {
    if (timer.current) clearTimeout(timer.current)
    timer.current = null
    const { css: value, site: s } = latest.current
    if (s)
      window.signet
        .saveBoost(s, { css: value })
        .then(() => setSaved(true))
        .catch(() => {})
  }, [])

  useEffect(
    () =>
      window.signet.onBoostState((next) => {
        stateRef.current = next
        setSt(next)
        // Never overwrite the input: neither while the field has focus, nor when a save is pending.
        const typing = document.activeElement === area.current || timer.current !== null
        if (!typing) {
          latest.current.css = next.boost.css
          setCss(next.boost.css)
        }
      }),
    []
  )
  useEffect(() => {
    if (!saved) return
    const t = setTimeout(() => setSaved(false), 1200)
    return () => clearTimeout(t)
  }, [saved])
  useEffect(() => () => void (timer.current && clearTimeout(timer.current)), [])
  // Window closing with pending input: save it before leaving.
  useEffect(() => {
    const onLeave = () => {
      if (timer.current) flush()
    }
    window.addEventListener('beforeunload', onLeave)
    window.addEventListener('pagehide', onLeave)
    return () => {
      window.removeEventListener('beforeunload', onLeave)
      window.removeEventListener('pagehide', onLeave)
    }
  }, [flush])

  const edit = (value: string, immediate = false) => {
    latest.current.css = value
    setCss(value)
    setSaved(false)
    if (timer.current) clearTimeout(timer.current)
    timer.current = setTimeout(flush, immediate ? 0 : 400)
  }

  const togglePreset = (id: string, body: string) => edit(hasBlock(css, id) ? withoutBlock(css, id) : withBlock(css, id, body), true)

  const onKeyDown = (e: React.KeyboardEvent<HTMLTextAreaElement>) => {
    if (e.key === 'Tab' && !e.shiftKey && !e.metaKey && !e.ctrlKey) {
      e.preventDefault()
      document.execCommand('insertText', false, '  ')
    }
  }

  if (!st) return <div className="boost" />
  const { boost, zapping, hasTab } = st
  const exists = boost.updatedAt > 0
  const signet = window.signet

  return (
    <div className="boost">
      <header className="boost-head">
        <div className="boost-title">
          <span className="b">Boost</span>
          <span className="s">{st.site}</span>
        </div>
        <button
          className={`switch ${boost.enabled ? 'on' : ''}`}
          role="switch"
          aria-checked={boost.enabled}
          title={boost.enabled ? t('boost.disable') : t('boost.enable')}
          onClick={() => signet.saveBoost(st.site, { enabled: !boost.enabled })}
        >
          <span className="knob" />
        </button>
      </header>

      <div className={`boost-body ${boost.enabled ? '' : 'off'}`}>
        <section>
          <h3>{t('boost.presets')}</h3>
          <div className="chips">
            {PRESETS.map((p) => {
              const on = hasBlock(css, p.id)
              return (
                <button key={p.id} className={`chip ${on ? 'on' : ''}`} onClick={() => togglePreset(p.id, p.css)}>
                  {on && <Check size={12} strokeWidth={2.4} />}
                  {t(p.label)}
                </button>
              )
            })}
          </div>
        </section>

        <section className="grow">
          <h3>
            CSS
            <span className={`saved ${saved ? 'show' : ''}`}>
              <Check size={11} /> {t('boost.saved')}
            </span>
          </h3>
          <textarea
            ref={area}
            className="boost-css"
            value={css}
            spellCheck={false}
            autoCapitalize="off"
            autoCorrect="off"
            placeholder={t('boost.cssPlaceholder', { site: st.site })}
            onChange={(e) => edit(e.target.value)}
            onKeyDown={onKeyDown}
            onBlur={() => {
              // Right after saving, the state is still the old one: do not resync from it.
              if (timer.current) return flush()
              const s = stateRef.current
              if (s) {
                latest.current.css = s.boost.css
                setCss(s.boost.css)
              }
            }}
          />
        </section>

        <section>
          <h3>{t('boost.hidden')}</h3>
          <button
            className={`zap-btn ${zapping ? 'on' : ''}`}
            disabled={!hasTab && !zapping}
            title={hasTab ? undefined : t('boost.zapNeedsTab')}
            onClick={() => (zapping ? signet.stopZap() : signet.startZap(st.site))}
          >
            <Crosshair size={14} />
            {zapping ? t('boost.zapDone') : t('boost.zapStart')}
          </button>
          {zapping && <p className="hint">{t('boost.zapHint')}</p>}
          {boost.zapped.length > 0 ? (
            <ul className="zapped">
              {boost.zapped.map((sel) => (
                <li key={sel}>
                  <code title={sel}>{sel}</code>
                  <button className="icon sm" title={t('boost.unhide')} onClick={() => signet.unzap(st.site, sel)}>
                    <X size={13} />
                  </button>
                </li>
              ))}
            </ul>
          ) : (
            !zapping && <p className="hint">{t('boost.noneHidden')}</p>
          )}
        </section>
      </div>

      <footer className="boost-foot">
        {confirming ? (
          <InlineConfirm
            btnClass="foot-btn"
            messageClass=""
            message={t('boost.confirmDelete', { site: st.site })}
            confirmLabel={t('boost.delete')}
            onCancel={() => setConfirming(false)}
            onConfirm={() => {
              setConfirming(false)
              latest.current.css = ''
              if (timer.current) clearTimeout(timer.current)
              timer.current = null
              void signet.deleteBoost(st.site)
            }}
          />
        ) : (
          <button className="foot-btn danger" disabled={!exists} onClick={() => setConfirming(true)}>
            <Trash2 size={13} />
            {t('boost.deleteBoost')}
          </button>
        )}
      </footer>
    </div>
  )
}

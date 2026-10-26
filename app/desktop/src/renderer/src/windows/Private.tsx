import { useEffect, useRef, useState } from 'react'
import { ArrowLeft, ArrowRight, Plus, RotateCw, VenetianMask, Volume2, X } from 'lucide-react'
import type { PrivateState } from '@shared/types'
import { useTranslation } from 'react-i18next'
import { Favicon } from '../ui/Favicon'
import { domainOf } from '../lib/util'

/**
 * Route #private: private browsing window, in the main window's layout: tab row
 * on top, or sidebar (tabs only, same width) on the left. The card carries the address bar;
 * the displayed tab's web view is placed by main under this bar.
 */
export function Private() {
  const { t } = useTranslation()
  const [st, setSt] = useState<PrivateState | null>(null)
  const [editing, setEditing] = useState(false)
  const [draft, setDraft] = useState('')
  const input = useRef<HTMLInputElement>(null)
  const signet = window.signet
  const urlRef = useRef('')

  useEffect(() => {
    let pushed = false
    const off = window.signet.onPrivateState((s) => {
      pushed = true
      setSt(s)
    })
    // Starting state requested from main; a state pushed in the meantime is more recent.
    void window.signet.getPrivateState().then((s) => s && !pushed && setSt(s))
    return off
  }, [])
  useEffect(() => window.signet.onPrivateFocusAddress(() => edit()), [])

  const active = st?.tabs.find((tab) => tab.id === st.activeId)
  const url = st?.url ?? ''
  const empty = !!st && !url

  function edit() {
    setEditing(true)
    // The field may already be displayed: select everything again.
    requestAnimationFrame(() => input.current?.select())
  }

  // Empty tab displayed (new window, ⌘T): the address field takes focus.
  useEffect(() => {
    if (empty) edit()
    else setEditing(false)
  }, [st?.activeId, empty])

  // Last known URL, read without rerunning the effect below.
  useEffect(() => {
    urlRef.current = url
  }, [url])

  useEffect(() => {
    // Only entering edit mode copies the URL: a navigation in progress does not overwrite typing.
    if (editing) setDraft(urlRef.current)
  }, [editing])

  const submit = () => {
    if (!draft.trim()) return
    setEditing(false)
    void signet.privateNavigate(draft)
  }

  const domain = url ? domainOf(url) : ''
  const title = active?.title && active.title !== url ? active.title : ''

  const side = st?.sidebarWidth ?? null
  const vertical = side !== null

  /** A tab: pill in the top row, row in the sidebar. */
  const tabEl = (tab: PrivateState['tabs'][number]) => (
    <div
      key={tab.id}
      role="tab"
      aria-selected={tab.id === st?.activeId}
      tabIndex={tab.id === st?.activeId ? 0 : -1}
      onKeyDown={(e) => {
        if (e.target !== e.currentTarget) return
        const sib =
          e.key === 'ArrowDown' || e.key === 'ArrowRight'
            ? e.currentTarget.nextElementSibling
            : e.key === 'ArrowUp' || e.key === 'ArrowLeft'
              ? e.currentTarget.previousElementSibling
              : null
        if (sib instanceof HTMLElement && sib.getAttribute('role') === 'tab') sib.focus()
        else if (e.key === 'Enter' || e.key === ' ') void signet.privateActivate(tab.id)
        else if (e.key === 'Delete') void signet.privateCloseTab(tab.id)
        else return
        e.preventDefault()
      }}
      className={`${vertical ? 'row' : 'tchip day'}${tab.id === st?.activeId ? ' active' : ''}`}
      title={tab.title || tab.url || t('private.newTab')}
      onMouseDown={(e) => {
        if (e.button === 0) void signet.privateActivate(tab.id)
      }}
      onAuxClick={(e) => {
        if (e.button === 1) void signet.privateCloseTab(tab.id)
      }}
    >
      <Favicon src={tab.favicon} loading={tab.loading} />
      {tab.audible && <Volume2 size={12} className="priv-sound" />}
      <span className="title">{tab.title || (tab.url ? domainOf(tab.url) : t('private.newTab'))}</span>
      <button className="mini close" title={t('private.closeTab')} onMouseDown={(e) => e.stopPropagation()} onClick={() => void signet.privateCloseTab(tab.id)}>
        <X size={vertical ? 14 : 12} />
      </button>
    </div>
  )

  const badge = (
    <span className="priv-badge" title={t('private.badgeHint')}>
      <VenetianMask size={14} />
      {t('private.badge')}
    </span>
  )

  return (
    <>
      {vertical ? (
        <aside className="sidebar priv-side" style={{ width: side }}>
          <div className="sb-head">
            <span className="spacer" />
            {badge}
          </div>
          <div className="priv-list" role="tablist" aria-orientation="vertical">
            {st?.tabs.map(tabEl)}
            <button className="row new-tab" onClick={() => void signet.privateNewTab()}>
              <Plus size={16} />
              <span className="title">{t('private.newTab')}</span>
            </button>
          </div>
        </aside>
      ) : (
        <div className="priv-strip">
          <div className="priv-tabs" role="tablist">
            {st?.tabs.map(tabEl)}
            <button className="icon priv-plus" title={t('private.newTab')} onClick={() => void signet.privateNewTab()}>
              <Plus size={16} />
            </button>
          </div>
          {badge}
        </div>
      )}

      <div className="pane-shadow priv-card" style={vertical ? { inset: `var(--gap) var(--gap) var(--gap) ${side}px` } : undefined}>
        <div className="toolbar priv-bar">
          <button className="icon" title={t('common.back')} disabled={!st?.canGoBack} onClick={() => signet.goBack()}>
            <ArrowLeft size={17} />
          </button>
          <button className="icon" title={t('common.forward')} disabled={!st?.canGoForward} onClick={() => signet.goForward()}>
            <ArrowRight size={17} />
          </button>
          <button className="icon" title={st?.loading ? t('toolbar.stop') : t('toolbar.reload')} disabled={empty} onClick={() => signet.reload()}>
            {st?.loading ? <X size={16} /> : <RotateCw size={15} />}
          </button>
          <div className="address-line">
            {editing ? (
              <input
                ref={input}
                className="priv-input"
                autoFocus
                spellCheck={false}
                value={draft}
                placeholder={t('toolbar.addressPlaceholder')}
                onChange={(e) => setDraft(e.target.value)}
                onFocus={(e) => e.currentTarget.select()}
                onBlur={() => !empty && setEditing(false)}
                onKeyDown={(e) => {
                  if (e.nativeEvent.isComposing) return
                  if (e.key === 'Enter') submit()
                  else if (e.key === 'Escape') {
                    if (empty) setDraft('')
                    else setEditing(false)
                  }
                }}
              />
            ) : (
              <button className="address" onClick={edit}>
                {url ? (
                  <>
                    <span className="domain">{domain}</span>
                    {title && <span className="page">/ {title}</span>}
                  </>
                ) : (
                  <span className="page">{t('toolbar.addressPlaceholder')}</span>
                )}
              </button>
            )}
          </div>
        </div>
        {empty && (
          <div className="priv-empty">
            <VenetianMask size={40} strokeWidth={1.5} className="priv-empty-icon" />
            <h1>{t('private.emptyTitle')}</h1>
            <p>{t('private.emptyText')}</p>
            <p className="note">{t('private.emptyNote')}</p>
          </div>
        )}
      </div>
    </>
  )
}

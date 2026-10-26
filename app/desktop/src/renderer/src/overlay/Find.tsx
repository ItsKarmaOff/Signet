import { useEffect, useRef, useState } from 'react'
import type { KeyboardEvent } from 'react'
import { useTranslation } from 'react-i18next'
import { ChevronDown, ChevronUp, Search, X } from 'lucide-react'
import type { FindState } from '@shared/types'
import { useHue } from '../lib/util'

/** Route #find: 360×48 pill at the top right of the page. */
export function Find() {
  const { t } = useTranslation()
  const [st, setSt] = useState<FindState | null>(null)
  const [query, setQuery] = useState('')
  const [tick, setTick] = useState(0)
  const input = useRef<HTMLInputElement>(null)
  const last = useRef<FindState | null>(null)
  const typedAt = useRef(0)
  useHue(st?.hue)

  useEffect(
    () =>
      window.signet.onFindState((n) => {
        const p = last.current
        last.current = n
        const fresh = !p || p.session !== n.session
        if (fresh) setQuery(n.query)
        // ⌘F on an already open bar returns exactly the same state: we take the focus back
        // (except mid-typing, where the main process echo must not select the text).
        const resend = !fresh && p.query === n.query && p.active === n.active && p.matches === n.matches
        if (fresh || (resend && Date.now() - typedAt.current > 400)) setTick((t) => t + 1)
        setSt(n)
      }),
    []
  )

  // The pill is remounted on every opening (session) to replay its entrance; if the view is still detached
  // from the window (hidden page), we wait until it is shown, otherwise the animation plays to nobody.
  const session = st?.session ?? null
  const closing = !!st?.closing
  const [live, setLive] = useState<number | null>(null)
  useEffect(() => {
    if (session === null || closing) return
    if (document.visibilityState === 'visible') return setLive(session)
    const go = () => setLive(session)
    const id = setTimeout(go, 150)
    const onVisible = () => {
      if (document.visibilityState !== 'visible') return
      clearTimeout(id)
      go()
    }
    document.addEventListener('visibilitychange', onVisible)
    return () => {
      clearTimeout(id)
      document.removeEventListener('visibilitychange', onVisible)
    }
  }, [session, closing])

  useEffect(() => {
    const el = input.current
    if (!el) return
    el.focus()
    el.select()
  }, [tick, live])

  const signet = window.signet
  const onKey = (e: KeyboardEvent) => {
    if (e.nativeEvent.isComposing) return
    if (e.key === 'Escape') {
      e.preventDefault()
      void signet.closeFind()
    } else if (e.key === 'Enter') {
      e.preventDefault()
      if (query) void signet.find(query, { forward: !e.shiftKey, findNext: true })
    }
  }

  const has = !!query && !!st
  const none = has && st.matches === 0
  const count = !has ? '' : none ? t('find.noResults') : t('find.count', { active: st.active, total: st.matches })
  const nav = (forward: boolean) => query && signet.find(query, { forward, findNext: true })

  if (live === null) return null
  return (
    <div key={live} className={`find-pill${closing || live !== session ? ' closing' : ''}`}>
      <Search size={15} className="find-ico" />
      <input
        ref={input}
        autoFocus
        value={query}
        placeholder={t('find.placeholder')}
        spellCheck={false}
        onKeyDown={onKey}
        onChange={(e) => {
          typedAt.current = Date.now()
          setQuery(e.target.value)
          void signet.find(e.target.value, { forward: true, findNext: false })
        }}
      />
      <span className={`find-count${none ? ' none' : ''}`}>{count}</span>
      <button className="icon sm" title={t('find.previous')} disabled={!has || none} onClick={() => nav(false)}>
        <ChevronUp size={16} />
      </button>
      <button className="icon sm" title={t('find.next')} disabled={!has || none} onClick={() => nav(true)}>
        <ChevronDown size={16} />
      </button>
      <button className="icon sm" title={t('common.close')} onClick={() => signet.closeFind()}>
        <X size={15} />
      </button>
    </div>
  )
}

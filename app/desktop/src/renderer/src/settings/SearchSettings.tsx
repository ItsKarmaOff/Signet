import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { Pencil, Trash2 } from 'lucide-react'
import type { SettingsInfo } from '@shared/types'
import {
  engineFavicon,
  normalizeKeyword,
  resolveEngines,
  validateEngineDraft,
  type CustomEngine,
  type EngineDraftError,
  type EngineSpec,
  type KeywordOverrides
} from '@signet/shared/searchengines'
import { Favicon } from '../ui/Favicon'

interface Draft {
  id: string | null
  name: string
  keyword: string
  template: string
  suggest: string
}
const EMPTY: Draft = { id: null, name: '', keyword: '', template: '', suggest: '' }
const ERROR_KEYS = {
  name: 'settings.search.errorName',
  keyword: 'settings.search.errorKeyword',
  keywordTaken: 'settings.search.errorKeywordTaken',
  template: 'settings.search.errorTemplate',
  suggest: 'settings.search.errorSuggest'
} as const satisfies Record<EngineDraftError, string>

/** Keyword field of one engine: saved on blur or Enter; empty or invalid goes back to the current keyword. */
function KeywordInput({
  engine,
  onCommit,
  label
}: {
  engine: EngineSpec
  onCommit: (k: string) => (typeof ERROR_KEYS)[EngineDraftError] | null
  label: string
}) {
  const [draft, setDraft] = useState<string | null>(null)
  const [error, setError] = useState<string | null>(null)
  const { t } = useTranslation()
  const commit = () => {
    if (draft === null) return
    const err = onCommit(draft)
    setError(err ? t(err) : null)
    if (!err) setDraft(null)
  }
  return (
    <>
      <input
        className="st-input st-kw"
        style={{ flex: 'none', width: 96 }}
        value={draft ?? engine.keyword}
        aria-label={label}
        spellCheck={false}
        maxLength={64}
        onChange={(e) => (setDraft(e.target.value), setError(null))}
        onBlur={commit}
        onKeyDown={(e) => {
          if (e.key === 'Enter') commit()
          else if (e.key === 'Escape') {
            setDraft(null)
            setError(null)
          }
        }}
      />
      {error && (
        <span className="st-hint st-err" role="alert">
          {error}
        </span>
      )}
    </>
  )
}

/** Settings › Search engines: keywords for the command bar, site searches and custom engines. */
export function SearchSettings({ info }: { info: SettingsInfo }) {
  const { t, i18n } = useTranslation()
  const { settings } = info
  const custom = settings.searchEngines ?? []
  const overrides = settings.searchKeywords ?? {}
  const engines = resolveEngines(settings, i18n.language)
  const [draft, setDraft] = useState<Draft | null>(null)
  const [error, setError] = useState<string | null>(null)
  const save = (patch: Parameters<typeof window.signet.updateSettings>[0]) => void window.signet.updateSettings(patch)

  const setOverride = (e: EngineSpec, change: { keyword?: string; disabled?: boolean }) => {
    const next: KeywordOverrides = { ...overrides }
    const cur = { ...next[e.id] }
    if ('keyword' in change) {
      if (change.keyword && change.keyword !== e.defaultKeyword) cur.keyword = change.keyword
      else delete cur.keyword
    }
    if ('disabled' in change) {
      if (change.disabled) cur.disabled = true
      else delete cur.disabled
    }
    if (cur.keyword || cur.disabled) next[e.id] = cur
    else delete next[e.id]
    save({ searchKeywords: next })
  }

  const commitKeyword = (e: EngineSpec, value: string): (typeof ERROR_KEYS)[EngineDraftError] | null => {
    // Empty: back to the default keyword (built-ins and presets only).
    const k = value.trim() === '' && e.defaultKeyword ? e.defaultKeyword : normalizeKeyword(value)
    if (!k) return ERROR_KEYS.keyword
    if (engines.some((o) => o.id !== e.id && o.keyword === k)) return ERROR_KEYS.keywordTaken
    if (e.source === 'custom') save({ searchEngines: custom.map((c) => (c.id === e.id ? { ...c, keyword: k } : c)) })
    else setOverride(e, { keyword: k })
    return null
  }

  const submit = () => {
    if (!draft) return
    const err = validateEngineDraft(draft, engines, draft.id ?? undefined)
    if (err) return setError(t(ERROR_KEYS[err]))
    const suggest = draft.suggest.trim()
    const entry: CustomEngine = {
      id: draft.id ?? `custom-${crypto.randomUUID().replace(/-/g, '').slice(0, 12)}`,
      name: draft.name.trim(),
      keyword: normalizeKeyword(draft.keyword)!,
      template: draft.template.trim(),
      ...(suggest && { suggest })
    }
    save({ searchEngines: draft.id ? custom.map((c) => (c.id === draft.id ? entry : c)) : [...custom, entry] })
    setDraft(null)
    setError(null)
  }

  const field = (key: keyof Omit<Draft, 'id'>, label: string, placeholder: string) => (
    <input
      className="st-input"
      value={draft![key]}
      placeholder={placeholder}
      aria-label={label}
      spellCheck={false}
      onChange={(e) => (setDraft({ ...draft!, [key]: e.target.value }), setError(null))}
    />
  )

  return (
    <>
      <p className="st-note">{t('settings.search.intro')}</p>
      <h3 className="st-sub">{t('settings.search.engines')}</h3>
      <div className="st-group">
        {engines.map((e) => (
          <div key={e.id} className="st-row">
            <Favicon src={engineFavicon(e)} size={16} />
            <div className="st-label">
              <span>{e.name}</span>
              <span className="st-hint">
                {t(`settings.search.${e.source}`)}
                {e.id === (settings.searchEngine ?? 'google') && ` · ${t('settings.search.default')}`}
              </span>
            </div>
            <KeywordInput engine={e} label={t('settings.search.keywordFor', { name: e.name })} onCommit={(v) => commitKeyword(e, v)} />
            {e.source === 'preset' && (
              <button
                className={`switch ${e.enabled ? 'on' : ''}`}
                role="switch"
                aria-checked={e.enabled}
                aria-label={t('settings.search.enabledFor', { name: e.name })}
                onClick={() => setOverride(e, { disabled: e.enabled })}
              >
                <span className="knob" />
              </button>
            )}
            {e.source === 'custom' && (
              <>
                <button
                  className="st-btn"
                  onClick={() => (setDraft({ id: e.id, name: e.name, keyword: e.keyword, template: e.template, suggest: e.suggest ?? '' }), setError(null))}
                >
                  <Pencil size={13} />
                  {t('settings.search.edit')}
                </button>
                <button className="st-btn" onClick={() => save({ searchEngines: custom.filter((c) => c.id !== e.id) })}>
                  <Trash2 size={13} />
                  {t('settings.search.remove')}
                </button>
              </>
            )}
          </div>
        ))}
      </div>
      {draft ? (
        <form
          className="st-group"
          onSubmit={(e) => {
            e.preventDefault()
            submit()
          }}
        >
          <div className="st-row">{field('name', t('settings.search.name'), t('settings.search.namePlaceholder'))}</div>
          <div className="st-row">{field('keyword', t('settings.search.keyword'), t('settings.search.keyword'))}</div>
          <div className="st-row">{field('template', t('settings.search.url'), t('settings.search.urlPlaceholder'))}</div>
          <div className="st-row">{field('suggest', t('settings.search.suggestUrl'), t('settings.search.suggestPlaceholder'))}</div>
          <p className="st-note">{t('settings.search.formHint')}</p>
          {error && (
            <p className="st-note st-err" role="alert">
              {error}
            </p>
          )}
          <div className="st-row">
            <button className="st-btn" type="button" onClick={() => (setDraft(null), setError(null))}>
              {t('settings.search.cancel')}
            </button>
            <button className="st-btn solid" type="submit">
              {t('settings.search.save')}
            </button>
          </div>
        </form>
      ) : (
        <div className="st-group">
          <div className="st-row">
            <button className="st-btn" onClick={() => setDraft(EMPTY)}>
              {t('settings.search.add')}
            </button>
          </div>
        </div>
      )}
    </>
  )
}

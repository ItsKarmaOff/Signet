import { useCallback, useEffect, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { ArrowDown, ArrowUp, Download, Paintbrush, Pencil, Plus, Trash2, Upload } from 'lucide-react'
import { InlineConfirm } from '../ui/InlineConfirm'
import type { InterfaceMod, ModsInfo } from '@shared/types'

/** Editor draft (`id` null = new mod). */
interface Draft {
  id: string | null
  name: string
  css: string
}

/**
 * Settings › Mods: CSS stylesheets applied to the whole Signet interface (never to web pages). Saving applies
 * right away, in all windows, Settings included: the preview is the interface itself.
 */
export function ModSettings() {
  const { t } = useTranslation()
  const signet = window.signet
  const [data, setData] = useState<ModsInfo | null>(null)
  const [draft, setDraft] = useState<Draft | null>(null)
  const [confirming, setConfirming] = useState<string | null>(null)
  const area = useRef<HTMLTextAreaElement>(null)

  const load = useCallback(() => {
    void signet
      .listMods()
      .then(setData)
      .catch(() => {})
  }, [signet])
  useEffect(() => {
    load()
    const off = signet.onModsChanged(load)
    window.addEventListener('focus', load)
    return () => {
      off()
      window.removeEventListener('focus', load)
    }
  }, [signet, load])
  useEffect(() => {
    if (draft) area.current?.focus()
  }, [draft?.id]) // eslint-disable-line react-hooks/exhaustive-deps -- only when the editor opens

  if (!data) return null
  const off = data.disabled || data.safeMode

  const save = async () => {
    if (!draft) return
    await signet.saveMod(draft.id, draft.name, draft.css)
    setDraft(null)
  }

  const row = (mod: InterfaceMod, i: number) => (
    <div key={mod.id} className="st-row">
      <div className="st-label">
        <span>{mod.name}</span>
        <span className="st-hint">{t('settings.mods.lines', { count: mod.css.split('\n').filter((l) => l.trim()).length })}</span>
      </div>
      {confirming === mod.id ? (
        <InlineConfirm
          message={t('settings.mods.confirmDelete', { name: mod.name })}
          confirmLabel={t('settings.mods.delete')}
          onCancel={() => setConfirming(null)}
          onConfirm={() => {
            setConfirming(null)
            void signet.deleteMod(mod.id)
          }}
        />
      ) : (
        <>
          <button
            className="icon"
            title={t('settings.mods.up')}
            aria-label={t('settings.mods.up')}
            disabled={i === 0}
            onClick={() => signet.moveMod(mod.id, -1)}
          >
            <ArrowUp size={14} />
          </button>
          <button
            className="icon"
            title={t('settings.mods.down')}
            aria-label={t('settings.mods.down')}
            disabled={i === data.mods.length - 1}
            onClick={() => signet.moveMod(mod.id, 1)}
          >
            <ArrowDown size={14} />
          </button>
          <button className="icon" title={t('settings.mods.export')} aria-label={t('settings.mods.export')} onClick={() => signet.exportMod(mod.id)}>
            <Upload size={14} />
          </button>
          <button className="st-btn" onClick={() => setDraft({ id: mod.id, name: mod.name, css: mod.css })}>
            <Pencil size={13} />
            {t('settings.mods.edit')}
          </button>
          <button className="icon" title={t('settings.mods.delete')} aria-label={t('settings.mods.delete')} onClick={() => setConfirming(mod.id)}>
            <Trash2 size={15} />
          </button>
          <button
            className={`switch ${mod.enabled ? 'on' : ''}`}
            role="switch"
            aria-checked={mod.enabled}
            aria-label={t('settings.mods.toggle', { name: mod.name })}
            onClick={() => signet.setModEnabled(mod.id, !mod.enabled)}
          >
            <span className="knob" />
          </button>
        </>
      )}
    </div>
  )

  return (
    <>
      <div className="st-group">
        <div className="st-row">
          <div className="st-label">
            <span>{t('settings.mods.disableAll')}</span>
            <span className="st-hint">{t('settings.mods.disableAllHint')}</span>
          </div>
          <button
            className={`switch ${data.disabled ? 'on' : ''}`}
            role="switch"
            aria-checked={data.disabled}
            aria-label={t('settings.mods.disableAll')}
            onClick={() => signet.setModsDisabled(!data.disabled)}
          >
            <span className="knob" />
          </button>
        </div>
      </div>
      {data.safeMode && <p className="st-note">{t('settings.mods.safeMode')}</p>}

      <div className="st-shortcuts-head">
        <h3 className="st-sub">{t('settings.mods.list')}</h3>
        <span className="mod-spacer" />
        <button className="st-btn" onClick={() => void signet.importMod()}>
          <Download size={13} />
          {t('settings.mods.import')}
        </button>
        <button className="st-btn" onClick={() => setDraft({ id: null, name: '', css: '' })}>
          <Plus size={13} />
          {t('settings.mods.add')}
        </button>
      </div>

      {draft && (
        <div className="st-group mod-editor">
          <input
            className="st-input"
            value={draft.name}
            maxLength={60}
            placeholder={t('settings.mods.namePlaceholder')}
            aria-label={t('settings.mods.name')}
            onChange={(e) => setDraft({ ...draft, name: e.target.value })}
          />
          <textarea
            ref={area}
            className="boost-css mod-css"
            value={draft.css}
            spellCheck={false}
            autoCapitalize="off"
            autoCorrect="off"
            placeholder={t('settings.mods.cssPlaceholder')}
            aria-label="CSS"
            onChange={(e) => setDraft({ ...draft, css: e.target.value })}
            onKeyDown={(e) => {
              // ⌘S / Ctrl+S saves; Tab inserts two spaces instead of leaving the field.
              if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === 's') {
                e.preventDefault()
                void save()
              } else if (e.key === 'Tab' && !e.shiftKey) {
                e.preventDefault()
                const el = e.currentTarget
                const { selectionStart: a, selectionEnd: b } = el
                const css = `${draft.css.slice(0, a)}  ${draft.css.slice(b)}`
                setDraft({ ...draft, css })
                requestAnimationFrame(() => el.setSelectionRange(a + 2, a + 2))
              } else if (e.key === 'Escape') setDraft(null)
            }}
          />
          <div className="st-row st-actions">
            <span className="st-hint grow">{off ? t('settings.mods.offHint') : t('settings.mods.liveHint')}</span>
            <button className="st-btn" onClick={() => setDraft(null)}>
              {t('common.cancel')}
            </button>
            <button className="st-btn st-btn-accent" onClick={() => void save()}>
              {t('settings.mods.save')}
            </button>
          </div>
        </div>
      )}

      {data.mods.length === 0 && !draft ? (
        <div className="st-empty">
          <Paintbrush size={28} strokeWidth={1.5} />
          <p>{t('settings.mods.empty')}</p>
        </div>
      ) : (
        data.mods.length > 0 && <div className="st-group">{data.mods.map(row)}</div>
      )}
      <p className="st-note">{t('settings.mods.help')}</p>
    </>
  )
}

import { useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { ArrowDown, ArrowUp, X } from 'lucide-react'

/** Languages offered when adding (BCP 47 code). */
const CHOICES = [
  'fr-FR',
  'fr-CA',
  'fr-BE',
  'fr-CH',
  'en-US',
  'en-GB',
  'en-CA',
  'en-AU',
  'es-ES',
  'es-MX',
  'de-DE',
  'it-IT',
  'pt-PT',
  'pt-BR',
  'nl-NL',
  'ja-JP',
  'ko-KR',
  'zh-CN',
  'zh-TW',
  'ar',
  'ru-RU',
  'pl-PL',
  'sv-SE',
  'tr-TR'
]

/** Name of a language code in the interface language ("Français (France)"), otherwise the code itself. */
function nameOf(names: Intl.DisplayNames, code: string): string {
  try {
    const n = names.of(code)
    return n ? n.charAt(0).toUpperCase() + n.slice(1) : code
  } catch {
    return code
  }
}

/** Ordered list of the languages announced to sites: drag, arrows, removal, adding, back to system. */
export function LanguageSettings({ languages, onChange }: { languages: string[]; onChange: (languages: string[]) => void }) {
  const { t, i18n } = useTranslation()
  const names = useMemo(() => new Intl.DisplayNames([i18n.language], { type: 'language', languageDisplay: 'dialect' }), [i18n.language])
  const [drag, setDrag] = useState<number | null>(null)
  const [over, setOver] = useState<number | null>(null)

  const move = (from: number, to: number) => {
    if (from === to || to < 0 || to >= languages.length) return
    const next = [...languages]
    next.splice(to, 0, next.splice(from, 1)[0])
    onChange(next)
  }
  const available = CHOICES.filter((c) => !languages.includes(c))

  return (
    <div className="st-langs">
      <ul className="lang-list">
        {languages.map((code, i) => (
          <li
            key={code}
            className={`lang-item ${drag === i ? 'dragging' : ''} ${over === i && drag !== null && drag !== i ? 'over' : ''}`}
            draggable
            onDragStart={() => setDrag(i)}
            onDragOver={(e) => {
              e.preventDefault()
              setOver(i)
            }}
            onDrop={() => {
              if (drag !== null) move(drag, i)
              setDrag(null)
              setOver(null)
            }}
            onDragEnd={() => {
              setDrag(null)
              setOver(null)
            }}
          >
            <span className="lang-rank">{i + 1}</span>
            <span className="lang-name">{nameOf(names, code)}</span>
            <span className="lang-code">{code}</span>
            <button className="lang-btn" aria-label={t('settings.languages.up')} disabled={i === 0} onClick={() => move(i, i - 1)}>
              <ArrowUp size={14} />
            </button>
            <button className="lang-btn" aria-label={t('settings.languages.down')} disabled={i === languages.length - 1} onClick={() => move(i, i + 1)}>
              <ArrowDown size={14} />
            </button>
            <button
              className="lang-btn"
              aria-label={t('settings.languages.remove')}
              disabled={languages.length < 2}
              onClick={() => onChange(languages.filter((_, j) => j !== i))}
            >
              <X size={14} />
            </button>
          </li>
        ))}
      </ul>
      <div className="lang-actions">
        <select
          className="st-select"
          value=""
          aria-label={t('settings.languages.addLabel')}
          onChange={(e) => e.target.value && onChange([...languages, e.target.value])}
        >
          <option value="">{t('settings.languages.add')}</option>
          {available.map((c) => (
            <option key={c} value={c}>
              {nameOf(names, c)}
            </option>
          ))}
        </select>
        <button className="st-btn" onClick={() => onChange([])}>
          {t('settings.languages.reset')}
        </button>
      </div>
      <p className="st-note">{t('settings.languages.note')}</p>
    </div>
  )
}

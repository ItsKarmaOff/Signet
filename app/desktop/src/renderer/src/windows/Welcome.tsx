import { useCallback, useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { Check } from 'lucide-react'
import { LOCALES, resolveLocale } from '@signet/shared/i18n'
import type { Locale, UiLanguage } from '@signet/shared/i18n'
import { DEFAULT_SEARCH_ENGINE, SEARCH_ENGINES } from '@signet/shared/mobile/search'
import type { ImportKind, Settings, SettingsInfo } from '@shared/types'
import { useHue } from '../lib/util'

const STEPS = ['language', 'engine', 'look', 'import', 'privacy', 'done'] as const

/** Engine logos, bundled in the app: no request at first launch, and Qwant refuses to display its own elsewhere. */
const ENGINE_LOGOS: Record<string, string> = Object.fromEntries(
  Object.entries(import.meta.glob<string>('../assets/engines/*.png', { eager: true, import: 'default' })).map(([path, url]) => [
    path.replace(/^.*\/|\.png$/g, ''),
    url
  ])
)

/** An engine's logo; its initial if it has no logo. */
function EngineLogo({ id, name }: { id: string; name: string }) {
  const src = ENGINE_LOGOS[id]
  return (
    <span className="wl-mono" aria-hidden="true">
      {src ? <img src={src} alt="" width={18} height={18} draggable={false} /> : name[0]}
    </span>
  )
}
type Step = (typeof STEPS)[number]

/** Data imported in one click: passwords and extensions (keychain, downloads) stay in Settings. */
const QUICK_KINDS: ImportKind[] = ['spaces', 'bookmarks', 'history']

function Choice<T extends string>({
  label,
  value,
  options,
  onChange
}: {
  label: string
  value: T
  options: readonly (readonly [T, string])[]
  onChange: (v: T) => void
}) {
  return (
    <div className="seg-ctl wl-seg" role="radiogroup" aria-label={label}>
      {options.map(([v, text]) => (
        <button key={v} role="radio" aria-checked={v === value} className={v === value ? 'on' : ''} onClick={() => onChange(v)}>
          {text}
        </button>
      ))}
    </div>
  )
}

function Row({ label, hint, children }: { label: string; hint?: string; children: React.ReactNode }) {
  return (
    <div className="st-row">
      <div className="st-label">
        <span>{label}</span>
        {hint && <span className="st-hint">{hint}</span>}
      </div>
      {children}
    </div>
  )
}

/** Route #welcome: first-launch introduction. Choices apply right away, as in Settings. */
export function Welcome() {
  const { t, i18n } = useTranslation()
  const [info, setInfo] = useState<SettingsInfo | null>(null)
  const [index, setIndex] = useState(0)
  const [imports, setImports] = useState<Record<string, 'busy' | 'done' | 'failed'>>({})
  useHue(info?.hue)

  const refresh = useCallback(() => window.signet.getSettingsInfo().then(setInfo), [])
  useEffect(() => {
    const load = () => void refresh().catch(() => {})
    load()
    // Return from the system "default browser" window.
    window.addEventListener('focus', load)
    return () => window.removeEventListener('focus', load)
  }, [refresh])

  const update = useCallback(
    async (patch: Partial<Settings>) => {
      setInfo((prev) => (prev ? { ...prev, settings: { ...prev.settings, ...patch } } : prev))
      await window.signet.updateSettings(patch)
      await refresh()
    },
    [refresh]
  )

  const last = index === STEPS.length - 1
  const next = useCallback(() => (last ? void window.signet.welcomeFinish() : setIndex((i) => i + 1)), [last])
  const back = () => setIndex((i) => Math.max(0, i - 1))
  const skip = () => void window.signet.welcomeFinish()

  // Enter continues, Escape skips (Enter on a button or a list keeps its usual meaning).
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') skip()
      else if (e.key === 'Enter' && !(e.target instanceof HTMLElement && e.target.closest('button, select, a'))) next()
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [next])

  const runImport = async (id: string) => {
    setImports((m) => ({ ...m, [id]: 'busy' }))
    try {
      await window.signet.runImport(id, QUICK_KINDS)
      setImports((m) => ({ ...m, [id]: 'done' }))
    } catch {
      setImports((m) => ({ ...m, [id]: 'failed' }))
    }
  }

  const step: Step = STEPS[index]
  const settings = info?.settings
  const pref: UiLanguage = settings?.uiLanguage ?? 'system'
  const systemLocale = pref === 'system' ? (i18n.language as Locale) : resolveLocale('system', navigator.languages)
  const engine = settings?.searchEngine ?? DEFAULT_SEARCH_ENGINE
  const sources = info?.importSources.filter((s) => s.available && s.kinds.some((k) => QUICK_KINDS.includes(k))) ?? []

  return (
    <div className="welcome">
      <div className="wl-drag" />
      <main className="wl-main" key={step} aria-live="polite">
        <h1>{t(`welcome.${step}.title`)}</h1>
        <p className="wl-text">{t(`welcome.${step}.text`)}</p>
        {info && settings && (
          <div className="wl-body">
            {step === 'language' && (
              <div className="st-group">
                <Row label={t('welcome.language.label')}>
                  <select className="st-select" value={pref} onChange={(e) => void update({ uiLanguage: e.target.value as UiLanguage })}>
                    <option value="system">
                      {t('settings.general.uiLanguageSystem', { language: LOCALES.find((l) => l.id === systemLocale)?.name ?? systemLocale })}
                    </option>
                    {LOCALES.map((l) => (
                      <option key={l.id} value={l.id}>
                        {l.name}
                      </option>
                    ))}
                  </select>
                </Row>
              </div>
            )}
            {step === 'engine' && (
              <div className="wl-engines" role="radiogroup" aria-label={t('welcome.engine.title')}>
                {SEARCH_ENGINES.map((e) => (
                  <button
                    key={e.id}
                    role="radio"
                    aria-checked={e.id === engine}
                    className={`wl-engine${e.id === engine ? ' on' : ''}`}
                    onClick={() => void update({ searchEngine: e.id })}
                  >
                    <EngineLogo id={e.id} name={e.name} />
                    <span>{e.name}</span>
                    {e.id === engine && <Check size={15} strokeWidth={2.4} />}
                  </button>
                ))}
              </div>
            )}
            {step === 'look' && (
              <div className="st-group">
                <Row label={t('welcome.look.theme')}>
                  <Choice
                    label={t('welcome.look.theme')}
                    value={settings.theme ?? 'system'}
                    options={[
                      ['system', t('welcome.look.themeSystem')],
                      ['light', t('welcome.look.themeLight')],
                      ['dark', t('welcome.look.themeDark')]
                    ]}
                    onChange={(theme) => void update({ theme })}
                  />
                </Row>
                <Row label={t('welcome.look.layout')}>
                  <Choice
                    label={t('welcome.look.layout')}
                    value={settings.tabLayout ?? 'vertical'}
                    options={[
                      ['vertical', t('welcome.look.layoutSide')],
                      ['horizontal', t('welcome.look.layoutTop')]
                    ]}
                    onChange={(tabLayout) => void update({ tabLayout })}
                  />
                </Row>
              </div>
            )}
            {step === 'import' &&
              (sources.length === 0 ? (
                <p className="st-note">{t('welcome.import.none')}</p>
              ) : (
                <>
                  <div className="st-group">
                    {sources.map((src) => {
                      const state = imports[src.id]
                      return (
                        <Row key={src.id} label={src.title}>
                          {state === 'done' ? (
                            <span className="st-ok">
                              <Check size={14} strokeWidth={2.4} /> {t('welcome.import.done')}
                            </span>
                          ) : (
                            <button className="st-btn" disabled={state === 'busy'} onClick={() => void runImport(src.id)}>
                              {state === 'busy' ? t('welcome.import.importing') : t('welcome.import.run')}
                            </button>
                          )}
                        </Row>
                      )
                    })}
                  </div>
                  {Object.values(imports).includes('failed') && (
                    <p className="st-note st-err" role="alert">
                      {t('welcome.import.failed')}
                    </p>
                  )}
                  <p className="st-note">{t('welcome.import.hint')}</p>
                </>
              ))}
            {step === 'privacy' && (
              <div className="st-group">
                <Row label={t('welcome.privacy.adblock')} hint={t('welcome.privacy.adblockHint')}>
                  <button
                    className={`switch ${settings.adblock ? 'on' : ''}`}
                    role="switch"
                    aria-checked={settings.adblock}
                    aria-label={t('welcome.privacy.adblock')}
                    onClick={() => void update({ adblock: !settings.adblock })}
                  >
                    <span className="knob" />
                  </button>
                </Row>
                <Row
                  label={t('welcome.privacy.defaultBrowser')}
                  hint={info.packaged ? (info.defaultBrowser ? t('welcome.privacy.isDefault') : undefined) : t('welcome.privacy.installedOnly')}
                >
                  {info.defaultBrowser ? (
                    <span className="st-ok">
                      <Check size={14} strokeWidth={2.4} />
                    </span>
                  ) : (
                    <button className="st-btn" disabled={!info.packaged} onClick={() => void window.signet.setDefaultBrowser()}>
                      {t('welcome.privacy.makeDefault')}
                    </button>
                  )}
                </Row>
              </div>
            )}
            {step === 'done' && <p className="st-hint wl-hint">{t('welcome.done.hint')}</p>}
          </div>
        )}
      </main>
      <footer className="wl-foot">
        <button className="wl-link" onClick={skip} style={{ visibility: last ? 'hidden' : 'visible' }}>
          {t('welcome.skip')}
        </button>
        <div className="wl-dots" role="img" aria-label={t('welcome.step', { current: index + 1, total: STEPS.length })}>
          {STEPS.map((s, i) => (
            <span key={s} className={`wl-dot${i === index ? ' on' : i < index ? ' past' : ''}`} />
          ))}
        </div>
        <div className="wl-actions">
          {index > 0 && !last && (
            <button className="st-btn" onClick={back}>
              {t('welcome.back')}
            </button>
          )}
          <button className="wl-primary" onClick={next} autoFocus={last}>
            {last ? t('welcome.start') : t('welcome.next')}
          </button>
        </div>
      </footer>
    </div>
  )
}

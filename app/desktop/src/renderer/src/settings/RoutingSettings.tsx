import { useEffect, useState } from 'react'
import type { FormEvent } from 'react'
import { Trans, useTranslation } from 'react-i18next'
import { Signpost, Trash2 } from 'lucide-react'
import type { SettingsInfo, Space } from '@shared/types'

/** Settings › Routing: links of certain sites open in a given space. */
export function RoutingSettings({ info }: { info: SettingsInfo }) {
  const { t } = useTranslation()
  const signet = window.signet
  const [spaces, setSpaces] = useState<Space[]>([])
  const [pattern, setPattern] = useState('')
  const [target, setTarget] = useState('')

  // The list of spaces is not part of `SettingsInfo`: re-read on every update and when returning to the window.
  useEffect(() => {
    let alive = true
    const load = () =>
      signet
        .getState()
        .then((s) => alive && setSpaces(s.spaces))
        .catch(() => {})
    const onFocus = () => void load()
    void load()
    window.addEventListener('focus', onFocus)
    return () => {
      alive = false
      window.removeEventListener('focus', onFocus)
    }
  }, [signet, info])

  const chosen = spaces.find((s) => s.id === target)?.id ?? spaces[0]?.id ?? ''
  const valid = pattern.trim().length > 0 && chosen !== ''
  const add = (e: FormEvent) => {
    e.preventDefault()
    if (!valid) return
    void signet.addRoute(pattern.trim(), chosen)
    setPattern('')
  }
  const routes = info.settings.routes

  return (
    <>
      <p className="st-note">
        <Trans i18nKey="settings.routing.note" components={{ kbd: <kbd />, code: <code /> }} />
      </p>
      {routes.length === 0 ? (
        <div className="st-empty">
          <Signpost size={28} strokeWidth={1.5} />
          <p>{t('settings.routing.empty')}</p>
        </div>
      ) : (
        <div className="st-group">
          {routes.map((route) => {
            const space = spaces.find((s) => s.id === route.spaceId)
            return (
              <div key={route.id} className="st-row">
                <div className="st-label">
                  <span className="st-pattern">{route.pattern}</span>
                </div>
                <span className="st-target">
                  <span aria-hidden>→</span>
                  {space ? (
                    <>
                      <span className="st-emoji">{space.icon}</span> {space.name}
                    </>
                  ) : (
                    <span className="st-hint">{t('settings.routing.deletedSpace')}</span>
                  )}
                </span>
                <button
                  className="icon"
                  title={t('settings.routing.deleteRule')}
                  aria-label={t('settings.routing.deleteRuleFor', { pattern: route.pattern })}
                  onClick={() => signet.removeRoute(route.id)}
                >
                  <Trash2 size={15} />
                </button>
              </div>
            )
          })}
        </div>
      )}
      <form className="st-group st-add" onSubmit={add}>
        <div className="st-row">
          <input
            className="st-input"
            value={pattern}
            placeholder={t('settings.routing.placeholder')}
            aria-label={t('settings.routing.patternLabel')}
            spellCheck={false}
            autoCapitalize="off"
            onChange={(e) => setPattern(e.target.value)}
          />
          <select className="st-select" value={chosen} aria-label={t('settings.routing.spaceLabel')} onChange={(e) => setTarget(e.target.value)}>
            {spaces.map((s) => (
              <option key={s.id} value={s.id}>
                {s.icon} {s.name}
              </option>
            ))}
          </select>
          <button type="submit" className="st-btn" disabled={!valid}>
            {t('settings.routing.add')}
          </button>
        </div>
      </form>
    </>
  )
}

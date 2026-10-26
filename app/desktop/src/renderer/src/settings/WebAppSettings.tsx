import { useEffect, useState } from 'react'
import { Trans, useTranslation } from 'react-i18next'
import { AppWindow, FolderOpen, Pencil, Trash2 } from 'lucide-react'
import { InlineConfirm } from '../ui/InlineConfirm'
import type { WebAppsInfo } from '@shared/types'
import { domainOf } from '../lib/util'

/** Settings › Apps: sites installed as apps (open, rename, show the launcher, uninstall). */
export function WebAppSettings() {
  const { t } = useTranslation()
  const signet = window.signet
  const [info, setInfo] = useState<WebAppsInfo | null>(null)
  const [editing, setEditing] = useState<{ id: string; name: string } | null>(null)
  const [confirming, setConfirming] = useState<string | null>(null)

  useEffect(() => {
    let alive = true
    const load = () =>
      void signet
        .listWebApps()
        .then((i) => alive && setInfo(i))
        .catch(() => {})
    load()
    const off = signet.onWebAppsChanged(load)
    return () => {
      alive = false
      off()
    }
  }, [signet])

  if (!info) return null
  if (info.apps.length === 0) {
    return (
      <div className="st-empty">
        <AppWindow size={28} strokeWidth={1.5} />
        <p>{t('settings.apps.empty1')}</p>
        <p>
          <Trans i18nKey="settings.apps.empty2" components={{ b: <b /> }} />
        </p>
      </div>
    )
  }

  const saveName = () => {
    if (editing?.name.trim()) void signet.renameWebApp(editing.id, editing.name)
    setEditing(null)
  }

  return (
    <>
      <p className="st-note">{t('settings.apps.intro', { folder: info.folder })}</p>
      <div className="st-group">
        {info.apps.map((a) => (
          <div key={a.id} className="st-row wa-row">
            {a.icon ? <img className="wa-row-icon" src={a.icon} alt="" draggable={false} /> : <AppWindow size={28} strokeWidth={1.5} className="wa-row-icon" />}
            {editing?.id === a.id ? (
              <input
                className="st-input"
                value={editing.name}
                maxLength={60}
                autoFocus
                spellCheck={false}
                aria-label={t('settings.apps.rename')}
                onChange={(e) => setEditing({ id: a.id, name: e.target.value })}
                onBlur={saveName}
                onKeyDown={(e) => {
                  if (e.key === 'Enter') saveName()
                  else if (e.key === 'Escape') setEditing(null)
                }}
              />
            ) : (
              <div className="st-label">
                <span>{a.name}</span>
                <span className="st-hint">
                  {domainOf(a.startUrl)}
                  {a.space ? ` · ${t('settings.apps.space', { space: `${a.space.icon} ${a.space.name}` })}` : ''}
                  {a.launcher ? '' : ` · ${t('settings.apps.noLauncher')}`}
                </span>
              </div>
            )}
            {confirming === a.id ? (
              <InlineConfirm
                message={t('settings.apps.confirm', { name: a.name })}
                confirmLabel={t('settings.apps.remove')}
                onCancel={() => setConfirming(null)}
                onConfirm={() => {
                  setConfirming(null)
                  void signet.removeWebApp(a.id)
                }}
              />
            ) : (
              editing?.id !== a.id && (
                <>
                  <button className="st-btn" onClick={() => signet.openWebApp(a.id)}>
                    {t('settings.apps.open')}
                  </button>
                  <button className="icon" title={t('settings.apps.rename')} onClick={() => setEditing({ id: a.id, name: a.name })}>
                    <Pencil size={14} />
                  </button>
                  <button
                    className="icon"
                    title={a.launcher ? t('settings.apps.reveal') : t('settings.apps.recreate')}
                    onClick={() => signet.revealWebApp(a.id)}
                  >
                    <FolderOpen size={15} />
                  </button>
                  <button className="icon" title={t('settings.apps.uninstall')} onClick={() => setConfirming(a.id)}>
                    <Trash2 size={15} />
                  </button>
                </>
              )
            )}
          </div>
        ))}
      </div>
      <p className="st-note">{t('settings.apps.howTo')}</p>
    </>
  )
}

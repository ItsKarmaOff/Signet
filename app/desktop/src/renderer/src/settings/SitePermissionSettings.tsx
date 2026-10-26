import { useCallback, useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { Trash2 } from 'lucide-react'
import type { SitePermissionEntry } from '@shared/types'

/** Settings › Privacy › Site permissions: view and forget remembered choices. */
export function SitePermissionSettings() {
  const { t } = useTranslation()
  const [entries, setEntries] = useState<SitePermissionEntry[] | null>(null)

  const load = useCallback(() => {
    window.signet
      .listSitePermissions()
      .then(setEntries)
      .catch(() => setEntries([]))
  }, [])
  useEffect(() => {
    load()
    window.addEventListener('focus', load)
    return () => window.removeEventListener('focus', load)
  }, [load])

  const reset = (origin?: string, permission?: string) => {
    void window.signet.resetSitePermissions(origin, permission).then(load, load)
  }
  const label = (permission: string) => {
    // Unknown Electron identifier: shown as is
    return t(`settings.privacy.permNames.${permission}`, { defaultValue: permission })
  }

  return (
    <>
      <h3 className="st-sub">{t('settings.privacy.sitePermissions')}</h3>
      {entries && entries.length > 0 ? (
        <div className="st-group">
          {entries.map((e) => (
            <div key={`${e.origin}|${e.permission}`} className="st-row">
              <div className="st-label">
                <span className="st-pattern">{e.origin.replace(/^\w+:\/\//, '')}</span>
                <span className="st-hint">
                  {label(e.permission)} · {t(e.decision === 'allow' ? 'settings.privacy.permAllowed' : 'settings.privacy.permDenied')}
                </span>
              </div>
              <button
                className="icon"
                title={t('settings.privacy.permReset')}
                aria-label={t('settings.privacy.permResetFor', { origin: e.origin, permission: label(e.permission) })}
                onClick={() => reset(e.origin, e.permission)}
              >
                <Trash2 size={15} />
              </button>
            </div>
          ))}
          <div className="st-row st-actions">
            <button className="st-btn danger" onClick={() => reset()}>
              <Trash2 size={13} />
              {t('settings.privacy.permResetAll')}
            </button>
          </div>
        </div>
      ) : (
        <p className="st-note">{entries ? t('settings.privacy.permEmpty') : ''}</p>
      )}
    </>
  )
}

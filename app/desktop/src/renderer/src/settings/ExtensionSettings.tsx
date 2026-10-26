import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { FolderOpen, Puzzle, RotateCw, Trash2 } from 'lucide-react'
import { InlineConfirm } from '../ui/InlineConfirm'
import type { SettingsInfo } from '@shared/types'

/** Settings › Extensions: installed extensions, removal and access to the store. */
export function ExtensionSettings({ info }: { info: SettingsInfo }) {
  const { t, i18n } = useTranslation()
  const signet = window.signet
  const [confirming, setConfirming] = useState<string | null>(null)
  const list = info.extensions
  const pinned = info.settings.pinnedExtensions ?? []
  const devMode = info.settings.devMode === true
  const time = (ms: number) => new Date(ms).toLocaleTimeString(i18n.language, { hour: '2-digit', minute: '2-digit', second: '2-digit' })

  return (
    <>
      <p className="st-note">{t('settings.extensions.note')}</p>
      <p className="st-note">{t('settings.extensions.orderHint')}</p>
      <div className="st-group">
        <div className="st-row">
          <div className="st-label">
            <span>{t('settings.extensions.store')}</span>
            <span className="st-hint">{t('settings.extensions.storeHint')}</span>
          </div>
          <button className="st-btn" onClick={() => signet.openExtensionStore()}>
            {t('settings.extensions.openStore')}
          </button>
        </div>
        {devMode && (
          <div className="st-row">
            <div className="st-label">
              <span>{t('settings.extensions.unpackedTitle')}</span>
              <span className="st-hint">{t('settings.extensions.unpackedHint')}</span>
            </div>
            <button className="st-btn" onClick={() => void signet.loadUnpackedExtension()}>
              <FolderOpen size={13} />
              {t('settings.extensions.loadUnpacked')}
            </button>
          </div>
        )}
      </div>
      {!devMode && <p className="st-note">{t('settings.extensions.unpackedDevMode')}</p>}
      {list.length === 0 ? (
        <div className="st-empty">
          <Puzzle size={28} strokeWidth={1.5} />
          <p>{t('settings.extensions.none')}</p>
        </div>
      ) : (
        <div className="st-group">
          {list.map((ext) => (
            <div key={ext.id || ext.unpacked?.path} className="st-row">
              {ext.icon ? <img className="st-ext-icon" src={ext.icon} alt="" /> : <Puzzle size={20} strokeWidth={1.5} />}
              <div className="st-label">
                <span>{ext.name}</span>
                {ext.version && <span className="st-hint">{t('settings.extensions.version', { version: ext.version })}</span>}
                {ext.unpacked && (
                  <>
                    <span className="st-hint">
                      {t('settings.extensions.unpackedLabel')}
                      {ext.unpacked.reloadedAt ? ` · ${t('settings.extensions.reloadedAt', { time: time(ext.unpacked.reloadedAt) })}` : ''}
                    </span>
                    <span className="st-hint st-ext-path" title={ext.unpacked.path}>
                      <bdi>{ext.unpacked.display}</bdi>
                    </span>
                  </>
                )}
                {ext.unpacked?.error && <span className="st-hint st-ext-error">{ext.unpacked.error}</span>}
              </div>
              {ext.unpacked && (
                <button className="st-btn" onClick={() => void signet.reloadUnpackedExtension(ext.unpacked!.path)}>
                  <RotateCw size={13} />
                  {t('settings.extensions.reload')}
                </button>
              )}
              {ext.hasAction && <span className="st-hint">{t('settings.extensions.showInToolbar')}</span>}
              {ext.hasAction && (
                <button
                  className={`switch ${pinned.includes(ext.id) ? 'on' : ''}`}
                  role="switch"
                  aria-checked={pinned.includes(ext.id)}
                  aria-label={t('settings.extensions.showInToolbar')}
                  title={t('settings.extensions.showInToolbar')}
                  onClick={() =>
                    signet.updateSettings({ pinnedExtensions: pinned.includes(ext.id) ? pinned.filter((x) => x !== ext.id) : [...pinned, ext.id] })
                  }
                >
                  <span className="knob" />
                </button>
              )}
              {confirming === (ext.id || ext.unpacked?.path) ? (
                <InlineConfirm
                  message={t(ext.unpacked ? 'settings.extensions.confirmUnpacked' : 'settings.extensions.confirm', { name: ext.name })}
                  confirmLabel={t('settings.extensions.remove')}
                  onCancel={() => setConfirming(null)}
                  onConfirm={() => {
                    setConfirming(null)
                    void (ext.unpacked ? signet.removeUnpackedExtension(ext.unpacked.path) : signet.removeExtension(ext.id))
                  }}
                />
              ) : (
                <button className="st-btn" onClick={() => setConfirming(ext.id || ext.unpacked?.path || null)}>
                  <Trash2 size={13} />
                  {t('settings.extensions.remove')}
                </button>
              )}
            </div>
          ))}
        </div>
      )}
    </>
  )
}

import { useEffect, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { Check, FolderOpen, HardDriveDownload, HardDriveUpload, History } from 'lucide-react'
import { BACKUP_SECTIONS } from '@shared/types'
import { reducedMotion } from '../lib/spring'
import type { BackupInfo, BackupMode, BackupSection, BackupSummary } from '@shared/types'

/** Minimum length of the backup password (same rules as main). */
const MIN_PASSWORD = 6

/** Checkbox list of the parts of a backup, with a three-state "Check all". */
function SectionList({
  counts,
  picked,
  onChange
}: {
  counts: Partial<Record<BackupSection, number>>
  picked: Set<BackupSection>
  onChange: (next: Set<BackupSection>) => void
}) {
  const { t } = useTranslation()
  const available = BACKUP_SECTIONS.filter((s) => counts[s] !== undefined)
  const all = available.length > 0 && available.every((s) => picked.has(s))
  const allRef = useRef<HTMLInputElement>(null)
  useEffect(() => {
    if (allRef.current) allRef.current.indeterminate = picked.size > 0 && !all
  }, [picked, all])
  const toggle = (s: BackupSection) => {
    const next = new Set(picked)
    if (!next.delete(s)) next.add(s)
    onChange(next)
  }
  const label = (s: BackupSection) => {
    const n = counts[s]
    const name = t(`backup.section.${s}`)
    // Settings: no count; file passwords: count unknown before decryption.
    return s === 'settings' || n === undefined || n < 0 ? name : t('backup.withCount', { name, count: n })
  }
  return (
    <div className="st-group">
      <label className="st-row st-check st-check-all">
        <input ref={allRef} type="checkbox" checked={all} onChange={() => onChange(all ? new Set() : new Set(available))} />
        <span>{t('backup.selectAll')}</span>
      </label>
      {available.map((s) => (
        <label key={s} className="st-row st-check">
          <input type="checkbox" checked={picked.has(s)} onChange={() => toggle(s)} />
          <span>{label(s)}</span>
          {s === 'settings' && <span className="st-hint">{t('backup.settingsHint')}</span>}
          {s === 'extensions' && <span className="st-hint">{t('backup.extensionsHint')}</span>}
        </label>
      ))}
    </div>
  )
}

/** Settings Backup section: export, import, restore a backup. */
export function BackupSettings() {
  const { t, i18n } = useTranslation()
  const signet = window.signet
  const [info, setInfo] = useState<BackupInfo | null>(null)

  // Export
  const [picked, setPicked] = useState<Set<BackupSection>>(new Set(BACKUP_SECTIONS.filter((s) => s !== 'passwords')))
  const [pw, setPw] = useState('')
  const [pw2, setPw2] = useState('')
  const [exporting, setExporting] = useState(false)
  const [exportMsg, setExportMsg] = useState<{ ok: boolean; text: string } | null>(null)

  // Import
  const [file, setFile] = useState<BackupSummary | null>(null)
  const [importPicked, setImportPicked] = useState<Set<BackupSection>>(new Set())
  const [mode, setMode] = useState<BackupMode>('add')
  const [importPw, setImportPw] = useState('')
  const [importing, setImporting] = useState(false)
  const [importErr, setImportErr] = useState<string | null>(null)
  // Backup loaded: bring the import block into view.
  const [fromSafety, setFromSafety] = useState(false)
  const importRef = useRef<HTMLDivElement>(null)
  useEffect(() => {
    if (!fromSafety || !file) return
    importRef.current?.scrollIntoView({ block: 'start', behavior: reducedMotion.matches ? 'auto' : 'smooth' })
    importRef.current?.focus({ preventScroll: true })
  }, [fromSafety, file])

  const refresh = () => {
    signet
      .getBackupInfo()
      .then(setInfo)
      .catch(() => {})
  }
  useEffect(refresh, [signet])

  const date = (ms: number) =>
    ms ? new Date(ms).toLocaleString(i18n.language, { day: 'numeric', month: 'long', year: 'numeric', hour: '2-digit', minute: '2-digit' }) : '—'

  const withPasswords = picked.has('passwords')
  const pwError =
    withPasswords && pw.length > 0 && pw.length < MIN_PASSWORD
      ? t('backup.passwordShort')
      : withPasswords && pw2 && pw !== pw2
        ? t('backup.passwordMismatch')
        : null
  const canExport = picked.size > 0 && !exporting && (!withPasswords || (pw.length >= MIN_PASSWORD && pw === pw2))

  const doExport = async () => {
    setExporting(true)
    setExportMsg(null)
    try {
      const err = await signet.exportBackup([...picked], withPasswords ? pw : null)
      setExportMsg(err ? { ok: false, text: err } : { ok: true, text: t('backup.exported') })
    } catch {
      setExportMsg({ ok: false, text: t('backup.exportFailed') })
    } finally {
      setExporting(false)
    }
  }

  /** File (or backup) read: all its parts checked, "add" mode by default ("replace" for a backup). */
  const loaded = (res: BackupSummary | string | null, safety = false) => {
    if (res === null) return
    if (typeof res === 'string') {
      setImportErr(res)
      setFile(null)
      return
    }
    setImportErr(null)
    setFile(res)
    setImportPw('')
    setMode(safety ? 'replace' : 'add')
    setFromSafety(safety)
    setImportPicked(new Set(BACKUP_SECTIONS.filter((s) => res.counts[s] !== undefined)))
  }

  const importNeedsPw = !!file && !file.systemSealed && importPicked.has('passwords')
  const canImport = !!file && importPicked.size > 0 && !importing && (!importNeedsPw || importPw.length > 0)
  const doImport = async () => {
    if (!file) return
    setImporting(true)
    setImportErr(null)
    try {
      // On success, the app restarts: the promise does not return.
      const err = await signet.applyBackup(file.token, [...importPicked], mode, importNeedsPw ? importPw : null)
      if (err) setImportErr(err)
    } catch {
      setImportErr(t('backup.importFailed'))
    } finally {
      setImporting(false)
    }
  }

  return (
    <>
      <p className="st-note st-backup-lead">{t('backup.lead')}</p>

      <h3 className="st-sub">{t('backup.exportHeading')}</h3>
      {info && (
        <SectionList
          counts={info.counts}
          picked={picked}
          onChange={(next) => {
            setPicked(next)
            setExportMsg(null)
          }}
        />
      )}
      {withPasswords && (
        <div className="st-group">
          <div className="st-row st-row-stack">
            <div className="st-label">
              <span>{t('backup.passwordLabel')}</span>
              <span className="st-hint">{t('backup.passwordHint')}</span>
            </div>
            <div className="st-backup-pw">
              <input
                className="st-input"
                type="password"
                autoComplete="new-password"
                placeholder={t('backup.passwordPlaceholder')}
                value={pw}
                onChange={(e) => setPw(e.target.value)}
              />
              <input
                className="st-input"
                type="password"
                autoComplete="new-password"
                placeholder={t('backup.passwordConfirm')}
                value={pw2}
                onChange={(e) => setPw2(e.target.value)}
              />
            </div>
            {pwError && <span className="st-err">{pwError}</span>}
          </div>
        </div>
      )}
      <div className="st-backup-actions">
        {exportMsg &&
          (exportMsg.ok ? (
            <span className="st-ok">
              <Check size={14} strokeWidth={2.4} /> {exportMsg.text}
            </span>
          ) : (
            <span className="st-err" role="alert">
              {exportMsg.text}
            </span>
          ))}
        <button className="st-btn" disabled={!canExport} onClick={() => void doExport()}>
          <HardDriveDownload size={14} />
          {exporting ? t('backup.exporting') : t('backup.export')}
        </button>
      </div>

      <div ref={importRef} tabIndex={-1} className="st-backup-import">
        <h3 className="st-sub">{t('backup.importHeading')}</h3>
      </div>
      {!file ? (
        <div className="st-group">
          <div className="st-row">
            <div className="st-label">
              <span>{t('backup.importLabel')}</span>
              <span className="st-hint">{t('backup.importHint')}</span>
            </div>
            <button className="st-btn" onClick={() => void signet.openBackup().then((r) => loaded(r))}>
              <HardDriveUpload size={14} />
              {t('backup.chooseFile')}
            </button>
          </div>
        </div>
      ) : (
        <>
          <div className="st-group">
            <div className="st-row">
              <div className="st-label">
                <span>{t('backup.fileFrom', { date: date(file.createdAt) })}</span>
                {file.appVersion && <span className="st-hint">{t('backup.fileVersion', { version: file.appVersion })}</span>}
              </div>
              <button className="st-btn" onClick={() => void signet.openBackup().then((r) => loaded(r))}>
                {t('backup.otherFile')}
              </button>
            </div>
          </div>
          {fromSafety && (
            <p className="st-note" role="status">
              {t('backup.safetyLoaded')}
            </p>
          )}
          <SectionList counts={file.counts} picked={importPicked} onChange={setImportPicked} />
          <div className="st-group">
            <div className="st-row st-row-stack">
              <div className="st-label">
                <span>{t('backup.modeLabel')}</span>
              </div>
              <div className="st-backup-mode" role="radiogroup" aria-label={t('backup.modeLabel')}>
                <label className={mode === 'add' ? 'on' : ''}>
                  <input type="radio" name="backup-mode" checked={mode === 'add'} onChange={() => setMode('add')} />
                  <span>
                    <strong>{t('backup.modeAdd')}</strong>
                    <span className="st-hint">{t('backup.modeAddHint')}</span>
                  </span>
                </label>
                <label className={mode === 'replace' ? 'on' : ''}>
                  <input type="radio" name="backup-mode" checked={mode === 'replace'} onChange={() => setMode('replace')} />
                  <span>
                    <strong>{t('backup.modeReplace')}</strong>
                    <span className="st-hint">{t('backup.modeReplaceHint')}</span>
                  </span>
                </label>
              </div>
            </div>
            {importNeedsPw && (
              <div className="st-row">
                <div className="st-label">
                  <span>{t('backup.importPasswordLabel')}</span>
                </div>
                <input
                  className="st-input"
                  type="password"
                  autoComplete="off"
                  placeholder={t('backup.passwordPlaceholder')}
                  value={importPw}
                  onChange={(e) => setImportPw(e.target.value)}
                />
              </div>
            )}
          </div>
          <div className="st-backup-actions">
            <button className="st-btn" onClick={() => setFile(null)}>
              {t('common.cancel')}
            </button>
            <button className={`st-btn${mode === 'replace' ? ' danger solid' : ''}`} disabled={!canImport} onClick={() => void doImport()}>
              {importing ? t('backup.importing') : t('backup.import')}
            </button>
          </div>
        </>
      )}
      {importErr && (
        <p className="st-note st-err" role="alert">
          {importErr}
        </p>
      )}
      <p className="st-note">{t('backup.restartNote')}</p>

      <h3 className="st-sub">{t('backup.safetyHeading')}</h3>
      <div className="st-group">
        {info && info.safety.length === 0 && (
          <div className="st-row">
            <span className="st-hint">{t('backup.safetyEmpty')}</span>
          </div>
        )}
        {info?.safety.map((s) => (
          <div key={s.name} className="st-row">
            <div className="st-label">
              <span>
                <History size={13} className="st-backup-icon" />
                {t('backup.safetyItem', { date: date(s.createdAt) })}
              </span>
            </div>
            <button className="st-btn" onClick={() => void signet.openSafetyBackup(s.name).then((r) => loaded(r, true))}>
              {t('backup.restore')}
            </button>
          </div>
        ))}
        <div className="st-row">
          <span className="st-hint grow">{t('backup.safetyHint')}</span>
          <button className="st-btn" onClick={() => void signet.revealSafetyBackups()}>
            <FolderOpen size={14} />
            {t('backup.revealSafety')}
          </button>
        </div>
      </div>
    </>
  )
}

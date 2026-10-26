import { useCallback, useEffect, useState } from 'react'
import type { FormEvent } from 'react'
import { useTranslation } from 'react-i18next'
import { Check, Copy, Download, Eye, EyeOff, KeyRound, Pencil, Plus, Trash2, Upload, X } from 'lucide-react'
import { InlineConfirm } from '../ui/InlineConfirm'
import { PASSWORD_EXTENSIONS } from '@shared/types'
import type { PasswordManager, PasswordsInfo, SavedLogin, SettingsInfo } from '@shared/types'

const MASK = '••••••••••'
const hostOf = (origin: string) => origin.replace(/^\w+:\/\//, '')

/** Draft of the add or edit form (`id` null = add). */
interface Draft {
  id: string | null
  origin: string
  username: string
  password: string
}

/** Settings › Passwords: manager choice, Signet password vault, never-saved sites. */
export function PasswordSettings({ info }: { info: SettingsInfo }) {
  const { t } = useTranslation()
  const signet = window.signet
  const { settings } = info
  const manager: PasswordManager = settings.passwordManager ?? 'signet'
  const ext = PASSWORD_EXTENSIONS.find((m) => m.id === manager)
  const installed = !!ext && info.extensions.some((e) => e.id === ext.extensionId)
  const [data, setData] = useState<PasswordsInfo | null>(null)
  const [query, setQuery] = useState('')
  const [shown, setShown] = useState<Record<string, string>>({})
  const [copied, setCopied] = useState<string | null>(null)
  const [confirming, setConfirming] = useState<string | null>(null)
  const [draft, setDraft] = useState<Draft | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [notice, setNotice] = useState<string | null>(null)
  const [noticeFailed, setNoticeFailed] = useState(false)
  const [loadFailed, setLoadFailed] = useState(false)
  const [installing, setInstalling] = useState(false)

  // The list is not part of `SettingsInfo` (never any passwords in the pushed state): re-read when returning to the window.
  const load = useCallback(() => {
    setShown({})
    signet
      .listPasswords()
      .then((d) => (setData(d), setLoadFailed(false)))
      .catch(() => setLoadFailed(true))
  }, [signet])
  // Revealed passwords: hidden after 30 s or when the window loses focus.
  useEffect(() => {
    if (!Object.keys(shown).length) return
    const timer = setTimeout(() => setShown({}), 30_000)
    const hide = () => setShown({})
    window.addEventListener('blur', hide)
    return () => {
      clearTimeout(timer)
      window.removeEventListener('blur', hide)
    }
  }, [shown])
  useEffect(() => {
    load()
    window.addEventListener('focus', load)
    return () => window.removeEventListener('focus', load)
  }, [load])

  const logins = data?.logins ?? []
  const q = query.trim().toLowerCase()
  const visible = q ? logins.filter((l) => l.origin.toLowerCase().includes(q) || l.username.toLowerCase().includes(q)) : logins

  const reveal = async (l: SavedLogin) => {
    if (shown[l.id] !== undefined) {
      setShown(({ [l.id]: _hidden, ...rest }) => rest)
      return
    }
    const password = await signet.revealPassword(l.id)
    if (password !== null) setShown((s) => ({ ...s, [l.id]: password }))
  }
  const copy = async (l: SavedLogin) => {
    if (await signet.copyPassword(l.id)) {
      setCopied(l.id)
      window.setTimeout(() => setCopied((c) => (c === l.id ? null : c)), 1500)
    }
  }
  const submit = async (e: FormEvent) => {
    e.preventDefault()
    if (!draft) return
    const err = await signet.savePassword(draft.id, draft.origin, draft.username, draft.password || null)
    if (err) return setError(err)
    setDraft(null)
    setError(null)
    if (draft.id) setShown(({ [draft.id as string]: _old, ...rest }) => rest)
    load()
  }
  const runImport = async () => {
    let res: Awaited<ReturnType<typeof signet.importPasswords>>
    try {
      res = await signet.importPasswords()
    } catch {
      res = { added: 0, existing: 0, error: 'read' }
    }
    if (res === null) return
    const message = res.error
      ? t(`settings.passwords.importError.${res.error}`)
      : res.existing > 0
        ? t('settings.passwords.importedSome', { added: res.added, existing: res.existing })
        : res.added > 0
          ? t('settings.passwords.imported', { count: res.added })
          : t('settings.passwords.importNothing')
    setNoticeFailed(!!res.error)
    setNotice(message)
    load()
  }
  const runExport = async () => {
    try {
      if (!(await signet.exportPasswords())) return
    } catch {
      setNoticeFailed(true)
      setNotice(t('settings.passwords.exportFailed'))
      return
    }
    setNoticeFailed(false)
    setNotice(t('settings.passwords.exported'))
  }

  const options: [PasswordManager, string][] = [
    ['signet', t('settings.passwords.managerSignet')],
    ...PASSWORD_EXTENSIONS.map((m): [PasswordManager, string] => [m.id, m.name]),
    ['off', t('settings.passwords.managerOff')]
  ]

  return (
    <>
      <div className="st-group">
        <div className="st-row">
          <div className="st-label">
            <span>{t('settings.passwords.manager')}</span>
            <span className="st-hint">
              {manager === 'signet'
                ? t('settings.passwords.managerSignetHint')
                : manager === 'off'
                  ? t('settings.passwords.managerOffHint')
                  : t('settings.passwords.managerExtHint', { name: ext?.name ?? '' })}
            </span>
          </div>
          <select
            className="st-select"
            value={manager}
            aria-label={t('settings.passwords.manager')}
            onChange={(e) => signet.updateSettings({ passwordManager: e.target.value as PasswordManager })}
          >
            {options.map(([id, label]) => (
              <option key={id} value={id}>
                {label}
              </option>
            ))}
          </select>
        </div>
        {ext && (
          <div className="st-row">
            <div className="st-label">
              <span>{t('settings.passwords.extension', { name: ext.name })}</span>
              <span className="st-hint">{installed ? t('settings.passwords.extensionInstalledHint') : t('settings.passwords.extensionMissingHint')}</span>
            </div>
            {installed ? (
              <span className="st-ok">
                <Check size={14} strokeWidth={2.4} /> {t('settings.passwords.installed')}
              </span>
            ) : (
              <button
                className="st-btn"
                disabled={installing}
                onClick={async () => {
                  setInstalling(true)
                  try {
                    await signet.installPasswordExtension(manager)
                  } finally {
                    setInstalling(false)
                  }
                }}
              >
                {installing ? t('settings.passwords.installing') : t('settings.passwords.install')}
              </button>
            )}
          </div>
        )}
        {manager === 'signet' && (
          <div className="st-row">
            <div className="st-label">
              <span>{t('settings.passwords.offer')}</span>
              <span className="st-hint">{t('settings.passwords.offerHint')}</span>
            </div>
            <button
              className={`switch ${settings.offerSavePasswords !== false ? 'on' : ''}`}
              role="switch"
              aria-checked={settings.offerSavePasswords !== false}
              aria-label={t('settings.passwords.offer')}
              onClick={() => signet.updateSettings({ offerSavePasswords: settings.offerSavePasswords === false })}
            >
              <span className="knob" />
            </button>
          </div>
        )}
      </div>

      {data && !data.encryption && (
        <p className="st-note st-err" role="alert">
          {t('settings.passwords.noEncryption')}
        </p>
      )}

      {(manager === 'signet' || logins.length > 0) && (
        <>
          <h3 className="st-sub">{t('settings.passwords.saved')}</h3>
          {manager !== 'signet' && <p className="st-note">{t('settings.passwords.leftover', { count: logins.length })}</p>}
          <div className="st-group">
            <div className="st-row">
              <input
                className="st-input"
                type="search"
                value={query}
                placeholder={t('settings.passwords.search')}
                aria-label={t('settings.passwords.search')}
                spellCheck={false}
                onChange={(e) => setQuery(e.target.value)}
              />
              <button
                className="st-btn"
                disabled={!data?.encryption}
                onClick={() => {
                  setError(null)
                  setDraft({ id: null, origin: '', username: '', password: '' })
                }}
              >
                <Plus size={13} />
                {t('settings.passwords.add')}
              </button>
              <button className="st-btn" disabled={!data?.encryption} title={t('settings.passwords.importHint')} onClick={runImport}>
                <Download size={13} />
                {t('settings.passwords.import')}
              </button>
              <button className="st-btn" disabled={logins.length === 0} onClick={runExport}>
                <Upload size={13} />
                {t('settings.passwords.export')}
              </button>
            </div>
            {notice && (
              <div className="st-row">
                {noticeFailed ? (
                  <span className="st-err grow" role="alert">
                    {notice}
                  </span>
                ) : (
                  <span className="st-ok grow">
                    <Check size={14} strokeWidth={2.4} /> {notice}
                  </span>
                )}
                <button className="icon" aria-label={t('common.close')} onClick={() => setNotice(null)}>
                  <X size={14} />
                </button>
              </div>
            )}
          </div>

          {draft && (
            <form
              className="st-group st-add"
              onSubmit={submit}
              onKeyDown={(e) => {
                if (e.key !== 'Escape') return
                e.preventDefault()
                e.stopPropagation()
                setDraft(null)
                setError(null)
              }}
            >
              <div className="st-row">
                <input
                  className="st-input"
                  value={draft.origin}
                  placeholder={t('settings.passwords.sitePlaceholder')}
                  aria-label={t('settings.passwords.site')}
                  spellCheck={false}
                  autoCapitalize="off"
                  autoFocus
                  onChange={(e) => setDraft({ ...draft, origin: e.target.value })}
                />
                <input
                  className="st-input"
                  value={draft.username}
                  placeholder={t('settings.passwords.username')}
                  aria-label={t('settings.passwords.username')}
                  spellCheck={false}
                  autoCapitalize="off"
                  autoComplete="off"
                  onChange={(e) => setDraft({ ...draft, username: e.target.value })}
                />
                <input
                  className="st-input"
                  type="password"
                  value={draft.password}
                  placeholder={draft.id ? t('settings.passwords.passwordUnchanged') : t('settings.passwords.password')}
                  aria-label={t('settings.passwords.password')}
                  autoComplete="new-password"
                  onChange={(e) => setDraft({ ...draft, password: e.target.value })}
                />
              </div>
              <div className="st-row st-actions">
                {error && (
                  <span className="st-err grow" role="alert">
                    {error}
                  </span>
                )}
                <button type="button" className="st-btn" onClick={() => setDraft(null)}>
                  {t('common.cancel')}
                </button>
                <button type="submit" className="st-btn">
                  {t('settings.passwords.saveLogin')}
                </button>
              </div>
            </form>
          )}

          {loadFailed && !data ? (
            <div className="st-empty" role="alert">
              <KeyRound size={28} strokeWidth={1.5} />
              <p>{t('settings.passwords.loadFailed')}</p>
              <button className="st-btn" onClick={load}>
                {t('common.retry')}
              </button>
            </div>
          ) : logins.length === 0 ? (
            <div className="st-empty">
              <KeyRound size={28} strokeWidth={1.5} />
              <p>{t('settings.passwords.empty')}</p>
            </div>
          ) : visible.length === 0 ? (
            <p className="st-note">{t('settings.passwords.noMatch')}</p>
          ) : (
            <div className="st-group">
              {visible.map((l) => (
                <div key={l.id} className="st-row">
                  <div className="st-label">
                    <span>{hostOf(l.origin)}</span>
                    <span className="st-hint">{l.username || t('settings.passwords.noUsername')}</span>
                  </div>
                  {confirming === l.id ? (
                    <InlineConfirm
                      message={t('settings.passwords.confirmDelete', { site: hostOf(l.origin), user: l.username || t('settings.passwords.noUsername') })}
                      confirmLabel={t('settings.passwords.delete')}
                      onCancel={() => setConfirming(null)}
                      onConfirm={() => {
                        setConfirming(null)
                        void signet.deletePassword(l.id).then(load)
                      }}
                    />
                  ) : (
                    <>
                      <code className="st-pw">{shown[l.id] ?? MASK}</code>
                      <button
                        className="icon"
                        title={shown[l.id] !== undefined ? t('settings.passwords.hide') : t('settings.passwords.show')}
                        aria-label={t(shown[l.id] !== undefined ? 'settings.passwords.hideFor' : 'settings.passwords.showFor', {
                          site: hostOf(l.origin),
                          user: l.username || t('settings.passwords.noUsername')
                        })}
                        onClick={() => void reveal(l)}
                      >
                        {shown[l.id] !== undefined ? <EyeOff size={15} /> : <Eye size={15} />}
                      </button>
                      <button
                        className="icon"
                        title={t('settings.passwords.copy')}
                        aria-label={t('settings.passwords.copyFor', { site: hostOf(l.origin), user: l.username || t('settings.passwords.noUsername') })}
                        onClick={() => void copy(l)}
                      >
                        {copied === l.id ? <Check size={15} /> : <Copy size={15} />}
                      </button>
                      <button
                        className="icon"
                        title={t('settings.passwords.edit')}
                        aria-label={t('settings.passwords.editFor', { site: hostOf(l.origin), user: l.username || t('settings.passwords.noUsername') })}
                        onClick={() => {
                          setError(null)
                          setDraft({ id: l.id, origin: l.origin, username: l.username, password: '' })
                        }}
                      >
                        <Pencil size={15} />
                      </button>
                      <button
                        className="icon"
                        title={t('settings.passwords.delete')}
                        aria-label={t('settings.passwords.deleteFor', { site: hostOf(l.origin), user: l.username || t('settings.passwords.noUsername') })}
                        onClick={() => setConfirming(l.id)}
                      >
                        <Trash2 size={15} />
                      </button>
                    </>
                  )}
                </div>
              ))}
            </div>
          )}
          {data && <p className="st-note">{t('settings.passwords.touchIdNote')}</p>}
        </>
      )}

      {(settings.neverSavePasswords ?? []).length > 0 && (
        <>
          <h3 className="st-sub">{t('settings.passwords.never')}</h3>
          <div className="st-group">
            {(settings.neverSavePasswords ?? []).map((origin) => (
              <div key={origin} className="st-row">
                <div className="st-label">
                  <span>{hostOf(origin)}</span>
                </div>
                <button
                  className="icon"
                  title={t('settings.passwords.neverRemove')}
                  aria-label={t('settings.passwords.neverRemoveFor', { site: hostOf(origin) })}
                  onClick={() => signet.updateSettings({ neverSavePasswords: (settings.neverSavePasswords ?? []).filter((o) => o !== origin) })}
                >
                  <Trash2 size={15} />
                </button>
              </div>
            ))}
          </div>
        </>
      )}
    </>
  )
}

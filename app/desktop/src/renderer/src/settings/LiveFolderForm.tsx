import { useEffect, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { GitPullRequest, Rss } from 'lucide-react'
import type { GithubQuery, LiveSource, Snapshot } from '@shared/types'

interface Props {
  s: Snapshot
  /** Folder whose source is being changed, or null to create one. */
  folderId: string | null
  onClose: () => void
}

/** Folder (pinned items of all spaces) by its id. */
function findLive(s: Snapshot, folderId: string | null): LiveSource | undefined {
  if (!folderId) return undefined
  const walk = (items: Snapshot['spaces'][number]['pinned']): LiveSource | undefined => {
    for (const item of items) {
      if (item.kind !== 'folder') continue
      if (item.id === folderId) return item.live?.source
      const inner = walk(item.items)
      if (inner) return inner
    }
    return undefined
  }
  for (const space of s.spaces) {
    const found = walk(space.pinned)
    if (found) return found
  }
  return undefined
}

/** Live folder form: a feed (or a site that announces one), or a GitHub query. */
export function LiveFolderForm({ s, folderId, onClose }: Props) {
  const { t } = useTranslation()
  const current = findLive(s, folderId)
  const [kind, setKind] = useState<LiveSource['kind']>(current?.kind ?? 'feed')
  const [url, setUrl] = useState(current?.kind === 'feed' ? current.url : '')
  const [query, setQuery] = useState<GithubQuery>(current?.kind === 'github' ? current.query : 'review-requested')
  const [token, setToken] = useState<boolean | null>(null)
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [tokenValue, setTokenValue] = useState('')
  const [tokenBusy, setTokenBusy] = useState(false)
  const [tokenError, setTokenError] = useState<string | null>(null)
  const urlRef = useRef<HTMLInputElement>(null)

  useEffect(() => {
    void window.signet
      .hasGithubToken()
      .then(setToken)
      .catch(() => setToken(false))
  }, [])
  useEffect(() => {
    if (kind === 'feed') urlRef.current?.focus()
  }, [kind])

  // Token entered here: saved (and verified) by the main process, as in Settings › General.
  const saveToken = async () => {
    const value = tokenValue.trim()
    if (!value || tokenBusy) return
    setTokenBusy(true)
    setTokenError(null)
    const message = await window.signet.setGithubToken(value).catch(() => null)
    setTokenBusy(false)
    if (message) return setTokenError(message)
    setTokenValue('')
    setToken(true)
  }

  const valid = kind === 'feed' ? url.trim().length > 0 : token === true
  const submit = async () => {
    if (!valid || busy) return
    setBusy(true)
    setError(null)
    const source: LiveSource = kind === 'feed' ? { kind: 'feed', url: url.trim() } : { kind: 'github', query }
    const message = await window.signet.saveLiveFolder(folderId, source).catch(() => t('folder.liveError', { error: '…' }))
    setBusy(false)
    if (message) setError(message)
    else onClose()
  }

  return (
    <form
      className="space-form live-form"
      onSubmit={(e) => {
        e.preventDefault()
        void submit()
      }}
      onKeyDown={(e) => e.key === 'Escape' && onClose()}
    >
      <strong className="live-title">{folderId ? t('liveForm.titleEdit') : t('liveForm.titleNew')}</strong>
      <div className="theme-seg" role="radiogroup" aria-label={t('liveForm.kind')}>
        <button type="button" role="radio" aria-checked={kind === 'feed'} className={kind === 'feed' ? 'on' : ''} onClick={() => setKind('feed')}>
          <Rss size={13} /> {t('liveForm.feed')}
        </button>
        <button type="button" role="radio" aria-checked={kind === 'github'} className={kind === 'github' ? 'on' : ''} onClick={() => setKind('github')}>
          <GitPullRequest size={13} /> {t('liveForm.github')}
        </button>
      </div>
      {kind === 'feed' ? (
        <>
          <input
            ref={urlRef}
            value={url}
            type="url"
            spellCheck={false}
            placeholder={t('liveForm.urlPlaceholder')}
            aria-label={t('liveForm.urlLabel')}
            onChange={(e) => setUrl(e.target.value)}
          />
          <p className="theme-hint">{t('liveForm.feedHint')}</p>
        </>
      ) : (
        <div className="live-choices" role="radiogroup">
          {(['review-requested', 'mine'] as const).map((q) => (
            <label key={q} className="live-choice">
              <input type="radio" name="live-query" checked={query === q} onChange={() => setQuery(q)} />
              <span>{q === 'mine' ? t('liveForm.mine') : t('liveForm.reviewRequested')}</span>
            </label>
          ))}
          {token === false && (
            <>
              <p className="theme-hint">{t('liveForm.noToken')}</p>
              <input
                type="password"
                autoComplete="off"
                spellCheck={false}
                value={tokenValue}
                placeholder={t('settings.general.githubTokenPlaceholder')}
                aria-label={t('settings.general.githubToken')}
                onChange={(e) => setTokenValue(e.target.value)}
                onKeyDown={(e) => {
                  if (e.key === 'Enter') {
                    e.preventDefault()
                    void saveToken()
                  }
                }}
              />
              <p className="theme-hint">
                <button type="button" className="link-btn" disabled={tokenBusy || !tokenValue.trim()} onClick={() => void saveToken()}>
                  {t('settings.general.githubTokenSave')}
                </button>{' '}
                <button type="button" className="link-btn" onClick={() => void window.signet.openSettings('general')}>
                  {t('liveForm.openSettings')}
                </button>
              </p>
              {tokenError && (
                <p className="live-error" role="alert">
                  {tokenError}
                </p>
              )}
            </>
          )}
        </div>
      )}
      {error && (
        <p className="live-error" role="alert">
          {error}
        </p>
      )}
      <div className="actions">
        <button type="button" onClick={onClose}>
          {t('common.cancel')}
        </button>
        <button type="submit" className="primary" disabled={!valid || busy}>
          {busy ? t('liveForm.checking') : folderId ? t('liveForm.save') : t('liveForm.create')}
        </button>
      </div>
    </form>
  )
}

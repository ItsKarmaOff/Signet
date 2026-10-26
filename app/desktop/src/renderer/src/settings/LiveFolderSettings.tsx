import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { Check } from 'lucide-react'

/** GitHub page to create a classic token, with the "repo" access already checked. */
const TOKEN_URL = 'https://github.com/settings/tokens/new?scopes=repo&description=Signet'

/** Settings › General: GitHub token of live folders (saved encrypted by the main process, never read back here). */
export function LiveFolderSettings() {
  const { t } = useTranslation()
  const [saved, setSaved] = useState<boolean | null>(null)
  const [value, setValue] = useState('')
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)

  useEffect(() => {
    void window.signet
      .hasGithubToken()
      .then(setSaved)
      .catch(() => setSaved(false))
  }, [])

  const save = async (token: string | null) => {
    setBusy(true)
    setError(null)
    // Channel failure (not a token refusal): say so rather than displaying "Token saved".
    let message: string | null
    try {
      message = await window.signet.setGithubToken(token)
    } catch {
      message = t('settings.general.githubTokenFailed')
    }
    setBusy(false)
    if (message) return setError(message)
    setValue('')
    setSaved(token !== null)
  }

  return (
    <>
      <h3 className="st-sub">{t('settings.general.liveFolders')}</h3>
      <div className="st-group">
        <div className="st-row st-row-stack">
          <div className="st-label">
            <span>{t('settings.general.githubToken')}</span>
            <span className="st-hint">{t('settings.general.githubTokenHint')}</span>
          </div>
          {saved ? (
            <div className="live-token-row">
              <span className="st-ok">
                <Check size={14} strokeWidth={2.4} /> {t('settings.general.githubTokenSet')}
              </span>
              <span className="grow" />
              <button className="st-btn danger" disabled={busy} onClick={() => void save(null)}>
                {t('settings.general.githubTokenRemove')}
              </button>
            </div>
          ) : (
            <form
              className="live-token-row"
              onSubmit={(e) => {
                e.preventDefault()
                if (value.trim()) void save(value.trim())
              }}
            >
              <input
                className="st-input"
                type="password"
                autoComplete="off"
                spellCheck={false}
                value={value}
                placeholder={t('settings.general.githubTokenPlaceholder')}
                aria-label={t('settings.general.githubToken')}
                onChange={(e) => setValue(e.target.value)}
              />
              <button type="submit" className="st-btn" disabled={busy || !value.trim()}>
                {t('settings.general.githubTokenSave')}
              </button>
              <a className="st-link" href={TOKEN_URL} target="_blank" rel="noreferrer">
                {t('settings.general.githubTokenCreate')}
              </a>
            </form>
          )}
          {error && (
            <span className="st-err" role="alert">
              {error}
            </span>
          )}
        </div>
      </div>
    </>
  )
}

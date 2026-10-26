import { Component, Fragment } from 'react'
import type { ErrorInfo, ReactNode } from 'react'
import { useTranslation } from 'react-i18next'

/** Automatic remounts tolerated within this time window; beyond that, offer to reload the interface. */
const MAX_RETRIES = 3
const RETRY_WINDOW = 30_000
const RETRY_DELAY = 300

/** Reports an interface error to main (log), without ever throwing an error itself. */
export function reportUiError(route: string, error: unknown, extra = ''): void {
  try {
    const e = error instanceof Error ? error : new Error(String(error))
    void window.signet.reportUiError?.(route, e.message, `${e.stack ?? ''}${extra ? `\n${extra}` : ''}`)?.catch(() => {})
  } catch {
    // the log is a convenience: nothing must break here
  }
}

function Fallback({ onReload }: { onReload: () => void }) {
  const { t } = useTranslation()
  return (
    <div className="ui-recovery" role="alert">
      <p>{t('recovery.message')}</p>
      <button type="button" className="ui-recovery-btn" onClick={onReload}>
        {t('recovery.reload')}
      </button>
    </div>
  )
}

interface State {
  failed: boolean
  generation: number
}

/**
 * Safeguard for each view: a render error no longer leaves the window empty (the pages, native views, would stay alone
 * on screen). The error is logged, the view remounts itself; if it fails again, a button reloads the interface.
 */
export class Recovery extends Component<{ route: string; children: ReactNode }, State> {
  state: State = { failed: false, generation: 0 }
  private failures: number[] = []
  private timer: ReturnType<typeof setTimeout> | null = null

  static getDerivedStateFromError(): Partial<State> {
    return { failed: true }
  }

  componentDidCatch(error: Error, info: ErrorInfo): void {
    reportUiError(this.props.route, error, info.componentStack ?? '')
    const now = Date.now()
    this.failures = [...this.failures.filter((at) => now - at < RETRY_WINDOW), now]
    if (this.failures.length > MAX_RETRIES) return // the reload button stays displayed
    if (this.timer) clearTimeout(this.timer)
    this.timer = setTimeout(() => {
      this.timer = null
      this.setState((s) => ({ failed: false, generation: s.generation + 1 }))
    }, RETRY_DELAY)
  }

  componentWillUnmount(): void {
    if (this.timer) clearTimeout(this.timer)
  }

  render(): ReactNode {
    if (this.state.failed) return this.failures.length > MAX_RETRIES ? <Fallback onReload={() => location.reload()} /> : null
    // New generation = fresh tree (no state left stuck in the error).
    return <Fragment key={this.state.generation}>{this.props.children}</Fragment>
  }
}

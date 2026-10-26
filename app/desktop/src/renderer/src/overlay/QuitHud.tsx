import { useEffect, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import type { QuitHudState } from '@shared/types'

/** Route #quit: bubble "Hold ⌘Q to quit" with its gauge. */
export function QuitHud() {
  const { t } = useTranslation()
  const [st, setSt] = useState<QuitHudState>({ progress: 0, holding: false })
  // Replays the appearance on every window display.
  const [shown, setShown] = useState(0)

  useEffect(() => window.signet.onQuitState(setSt), [])
  useEffect(() => {
    const on = () => document.visibilityState === 'visible' && setShown((n) => n + 1)
    document.addEventListener('visibilitychange', on)
    return () => document.removeEventListener('visibilitychange', on)
  }, [])

  return (
    <div className="quit-hud">
      <div className="quit-pill" key={shown} role="status">
        <div className="quit-title">{t('quit.title')}</div>
        <div className="quit-sub">{t('quit.again')}</div>
        <Gauge st={st} />
      </div>
    </div>
  )
}

/**
 * Until a key repeat proves a held press, the gauge runs at normal speed up to the expected repeat,
 * then slows down smoothly (it never stops dead) and gains at most KNEE more. Catch-up speed: × the normal speed.
 */
const KNEE = 0.1
const CATCH_UP = 2
/** Share used when the main process does not send the expected repeat delay. */
const DEFAULT_WAIT_SHARE = 0.4

/** Gauge target `x` (elapsed share of the hold) before the repeat: linear up to `xw`, then easing toward `xw + KNEE`. */
function waitingTarget(x: number, xw: number): number {
  return x <= xw ? x : xw + KNEE * Math.tanh((x - xw) / KNEE)
}

/**
 * Gauge: animated here, frame by frame, from the start of the press sent by the main process (a single message).
 * On release it returns toward zero from its current value (CSS transition).
 */
function Gauge({ st }: { st: QuitHudState }) {
  const ref = useRef<HTMLDivElement>(null)
  const live = useRef(0)
  const { holding, start, duration, wait, repeating } = st
  // `repeating` changes mid-press: the loop reads the latest value without restarting.
  const repeatRef = useRef(false)
  repeatRef.current = !!repeating

  useEffect(() => {
    const el = ref.current
    if (!el) return
    if (!holding || start === undefined || !duration) {
      el.style.transform = 'scaleX(0)'
      live.current = 0
      return
    }
    // Resumes from the current value (a return during the recoil has no jump): the transition is off during the press.
    const current = new DOMMatrixReadOnly(getComputedStyle(el).transform).a
    live.current = Number.isFinite(current) ? current : 0
    let raf = 0
    let last = performance.now()
    const frame = (now: number) => {
      const dt = now - last
      last = now
      const x = Math.max(0, (Date.now() - start) / duration)
      const xw = wait !== undefined ? wait / duration : DEFAULT_WAIT_SHARE
      const target = Math.min(1, repeatRef.current ? x : waitingTarget(x, xw))
      // Catches up to its target at most CATCH_UP times the normal speed: no jump when the repeat arrives late.
      live.current = Math.min(target, live.current + (dt * CATCH_UP) / duration)
      el.style.transform = `scaleX(${live.current})`
      if (live.current < 1) raf = requestAnimationFrame(frame)
    }
    raf = requestAnimationFrame(frame)
    return () => cancelAnimationFrame(raf)
  }, [holding, start, duration, wait])

  return <div ref={ref} className={`quit-gauge${holding ? ' holding' : ''}`} />
}

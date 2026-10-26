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
 * Until a key repeat proves a held press, the gauge only creeps (ease-in) up to CREEP at the expected
 * repeat time and stays there: a tap barely moves it. The first repeat then lets it fill up to 100% over the rest of the hold.
 * Catch-up speed: × the normal speed.
 */
const CREEP = 0.1
const CATCH_UP = 2
/** Share used when the main process does not send the expected repeat delay. */
const DEFAULT_WAIT_SHARE = 0.4

/** Gauge target at time `t` (ms since the press) for a hold of `d` ms with the first repeat expected at `w`. */
function gaugeTarget(t: number, d: number, w: number, repeating: boolean): number {
  if (!repeating) return CREEP * Math.min(1, t / w) ** 2
  return Math.min(1, CREEP + ((1 - CREEP) * Math.max(0, t - w)) / Math.max(1, d - w))
}

/**
 * Gauge: animated here, frame by frame, from the start of the press sent by the main process (a single message).
 * On release it freezes at once at its current value (no easing); a new press starts again from zero.
 */
function Gauge({ st }: { st: QuitHudState }) {
  const ref = useRef<HTMLDivElement>(null)
  const live = useRef(0)
  const lastStart = useRef<number | undefined>(undefined)
  const { holding, start, duration, wait, repeating } = st
  // `repeating` changes mid-press: the loop reads the latest value without restarting.
  const repeatRef = useRef(false)
  repeatRef.current = !!repeating

  useEffect(() => {
    const el = ref.current
    if (!el) return
    if (!holding || start === undefined || !duration) {
      // Released: the loop is cancelled and the gauge stays where it is (the pill is remounted on each display).
      return
    }
    // Same press resumed after a wrong release guess: continues from the frozen value; a new press starts from zero.
    if (lastStart.current !== start) live.current = 0
    lastStart.current = start
    let raf = 0
    let last = performance.now()
    const frame = (now: number) => {
      const dt = now - last
      last = now
      const t = Math.max(0, Date.now() - start)
      const w = Math.max(1, wait ?? duration * DEFAULT_WAIT_SHARE)
      const target = gaugeTarget(t, duration, w, repeatRef.current)
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

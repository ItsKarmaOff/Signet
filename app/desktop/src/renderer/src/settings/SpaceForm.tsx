import { useTranslation } from 'react-i18next'
import { useEffect, useRef, useState } from 'react'
import type { Space } from '@shared/types'
import { HUES, themeBackground } from '../lib/util'
import { EmojiPicker } from '../ui/EmojiPicker'

interface Props {
  /** Space to edit, or null to create one. */
  space: Space | null
  /** Space targeted at opening: if `space` disappears afterwards, we do not switch to creation. */
  editId?: string | null
  onClose: () => void
}

/** Small panel confined to the sidebar (web views would hide everything else). */
export function SpaceForm({ space, editId, onClose }: Props) {
  const { t } = useTranslation()
  const editRef = useRef(editId ?? space?.id ?? null)
  const [name, setName] = useState(space?.name ?? '')
  const [icon, setIcon] = useState(space?.icon ?? '✨')
  const [hue, setHue] = useState(space?.hue ?? HUES[Math.floor(Math.random() * HUES.length)])
  // Space background: solid color (`hue`) or gradient of 2 or 3 hues, in the chosen order.
  const [mode, setMode] = useState<'solid' | 'gradient'>(space?.theme ? 'gradient' : 'solid')
  const [hues, setHues] = useState<number[]>(space?.theme?.hues ?? [])
  const [grain, setGrain] = useState(space?.theme?.grain ?? 0)
  const [picking, setPicking] = useState(false)
  const [busy, setBusy] = useState(false)
  const busyRef = useRef(false)
  const [isolated, setIsolated] = useState(space?.profile === 'isolated')
  const nameRef = useRef<HTMLInputElement>(null)
  const emojiRef = useRef<HTMLButtonElement>(null)

  useEffect(() => nameRef.current?.focus(), [])
  // The edited space disappeared: close the form.
  useEffect(() => {
    if (editRef.current && !space) onClose()
  }, [space, onClose])

  const gradient = mode === 'gradient'
  const valid = name.trim().length > 0 && (!gradient || hues.length >= 2)
  /** Switches to gradient starting from the solid color and its neighbor on the palette. */
  const toGradient = () => {
    setMode('gradient')
    if (hues.length < 2) setHues([hue, HUES[(HUES.indexOf(hue) + 3) % HUES.length] ?? HUES[0]])
  }
  /** Gradient: a click adds the hue (the last one is replaced beyond 3), a second click removes it. */
  const toggleHue = (h: number) =>
    setHues((list) => (list.includes(h) ? list.filter((x) => x !== h) : list.length >= 3 ? [...list.slice(0, 2), h] : [...list, h]))
  const submit = async () => {
    if (!valid || busyRef.current) return
    busyRef.current = true // the state flag arrives too late for a double Enter
    setBusy(true)
    try {
      await save()
    } finally {
      busyRef.current = false
      setBusy(false)
    }
  }
  const save = async () => {
    // The edited space was deleted in the meantime: nothing to save.
    if (editRef.current && !space) return onClose()
    const draft = gradient
      ? { name: name.trim(), icon: icon || '✨', hue: hues[0], theme: { hues, grain } }
      : { name: name.trim(), icon: icon || '✨', hue, theme: null }
    const signet = window.signet
    if (space) {
      await signet.updateSpace(space.id, draft)
      if (isolated !== (space.profile === 'isolated')) await signet.setSpaceProfile(space.id, isolated ? 'isolated' : 'shared')
    } else {
      await signet.createSpace(draft)
      // The new space becomes the active space.
      if (isolated) await signet.setSpaceProfile((await signet.getState()).activeSpaceId, 'isolated')
    }
    onClose()
  }

  return (
    <form
      className="space-form"
      style={{ ['--h' as string]: gradient ? (hues[0] ?? hue) : hue }}
      onSubmit={(e) => {
        e.preventDefault()
        void submit()
      }}
      onKeyDown={(e) => {
        if (e.key !== 'Escape' || e.nativeEvent.isComposing) return
        if (picking) {
          setPicking(false)
          emojiRef.current?.focus()
        } else onClose()
      }}
    >
      <div className="field-row">
        <button
          type="button"
          ref={emojiRef}
          className={`emoji-btn${picking ? ' on' : ''}`}
          aria-label={t('spaceForm.pickEmoji')}
          aria-expanded={picking}
          onClick={() => setPicking((p) => !p)}
        >
          {icon}
        </button>
        <input
          ref={nameRef}
          value={name}
          maxLength={40}
          placeholder={t('spaceForm.namePlaceholder')}
          onChange={(e) => setName(e.target.value)}
          aria-label={t('spaceForm.nameLabel')}
        />
      </div>
      {picking && (
        <EmojiPicker
          onPick={(emoji) => {
            setIcon(emoji)
            setPicking(false)
            nameRef.current?.focus()
          }}
        />
      )}
      <div className="theme-row">
        <div className="theme-seg" role="radiogroup" aria-label={t('spaceForm.background')}>
          <button type="button" role="radio" aria-checked={!gradient} className={gradient ? '' : 'on'} onClick={() => setMode('solid')}>
            {t('spaceForm.solid')}
          </button>
          <button type="button" role="radio" aria-checked={gradient} className={gradient ? 'on' : ''} onClick={toGradient}>
            {t('spaceForm.gradient')}
          </button>
        </div>
        <span className="theme-preview" style={{ background: themeBackground(gradient ? { hues, grain } : undefined, hue) }} aria-hidden />
      </div>
      <div className={`swatches${gradient ? ' multi' : ''}`}>
        {HUES.map((h) => {
          const rank = hues.indexOf(h)
          const on = gradient ? rank >= 0 : h === hue
          return (
            <button
              type="button"
              key={h}
              className={on ? 'on' : ''}
              style={{ background: `hsl(${h} 70% 62%)` }}
              onClick={() => (gradient ? toggleHue(h) : setHue(h))}
              aria-label={t('spaceForm.hue', { hue: h })}
              aria-pressed={on}
            >
              {gradient && rank >= 0 && <span className="swatch-rank">{rank + 1}</span>}
            </button>
          )
        })}
      </div>
      {gradient && (
        <>
          {hues.length < 2 && <p className="theme-hint">{t('spaceForm.gradientHint')}</p>}
          <label className="grain-row">
            <span>{t('spaceForm.grain')}</span>
            <input
              type="range"
              min={0}
              max={100}
              step={5}
              value={Math.round(grain * 100)}
              onChange={(e) => setGrain(Number(e.target.value) / 100)}
              aria-valuetext={`${Math.round(grain * 100)} %`}
            />
          </label>
        </>
      )}
      <div className="profile-row">
        <div className="profile-text">
          <span className="profile-title">{t('spaceForm.isolated')}</span>
          <span className="profile-hint">{t('spaceForm.isolatedHint')}</span>
        </div>
        <button
          type="button"
          className={`switch ${isolated ? 'on' : ''}`}
          role="switch"
          aria-checked={isolated}
          aria-label={t('spaceForm.isolated')}
          onClick={() => setIsolated((on) => !on)}
        >
          <span className="knob" />
        </button>
      </div>
      <div className="actions">
        <button type="button" onClick={onClose}>
          {t('common.cancel')}
        </button>
        <button type="submit" className="primary" disabled={!valid || busy}>
          {space ? t('spaceForm.save') : t('spaceForm.create')}
        </button>
      </div>
    </form>
  )
}

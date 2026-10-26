import { Fragment, useCallback, useEffect, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { RotateCcw } from 'lucide-react'
import { InlineConfirm } from '../ui/InlineConfirm'
import type { SettingsInfo } from '@shared/types'
import { IS_MAC, platformKeys } from '@signet/shared/i18n'
import {
  FIXED_SHORTCUTS,
  SHORTCUTS,
  SHORTCUT_GROUPS,
  accelKey,
  accelLabel,
  defaultAccel,
  shortcutAccels,
  shortcutConflict,
  shortcutDef,
  takeOver,
  usableAccel,
  type ShortcutOverrides
} from '@shared/shortcuts'

/** Physical keys (`KeyboardEvent.code`) that are neither letters nor digits → Electron key. */
const CODE_KEYS: Record<string, string> = {
  Minus: '-',
  Equal: '=',
  BracketLeft: '[',
  BracketRight: ']',
  Backslash: '\\',
  Semicolon: ';',
  Quote: "'",
  Comma: ',',
  Period: '.',
  Slash: '/',
  Backquote: '`',
  Space: 'Space',
  Tab: 'Tab',
  Enter: 'Return',
  NumpadEnter: 'Return',
  ArrowUp: 'Up',
  ArrowDown: 'Down',
  ArrowLeft: 'Left',
  ArrowRight: 'Right',
  Home: 'Home',
  End: 'End',
  PageUp: 'PageUp',
  PageDown: 'PageDown',
  Delete: 'Delete',
  Insert: 'Insert'
}
const PUNCT = /^[-=[\]\\;',./`]$/
const MODIFIER_KEYS = new Set(['Meta', 'Control', 'Alt', 'Shift', 'CapsLock', 'Fn', 'OS', 'AltGraph'])

type LayoutMap = { get(code: string): string | undefined }

/** Electron key of a key press: the letter from the keyboard layout (AZERTY included), otherwise the physical key's. */
function keyOf(e: KeyboardEvent, layout: LayoutMap | null): string | null {
  const code = e.code
  if (/^F([1-9]|1[0-9]|2[0-4])$/.test(code)) return code
  // Keyboard layout first, for all keys (on AZERTY, M is `Semicolon`).
  const ch = layout?.get(code)
  if (ch && /^[a-z0-9]$/i.test(ch)) return ch.toUpperCase()
  if (ch && PUNCT.test(ch)) return ch
  if (/^(Key[A-Z]|Digit[0-9])$/.test(code)) return code.slice(-1)
  return CODE_KEYS[code] ?? null
}

/** Electron accelerator of a key press (⌘ → CmdOrCtrl on Mac, Ctrl → CmdOrCtrl elsewhere). */
function accelOf(e: KeyboardEvent, key: string): string {
  const mods = IS_MAC
    ? [e.ctrlKey && 'Ctrl', e.altKey && 'Alt', e.shiftKey && 'Shift', e.metaKey && 'CmdOrCtrl']
    : [e.altKey && 'Alt', e.shiftKey && 'Shift', e.ctrlKey && 'CmdOrCtrl', e.metaKey && 'Super']
  return [...mods.filter(Boolean), key].join('+')
}

/** No difference (stable reference for effect dependencies). */
const NO_OVERRIDES: ShortcutOverrides = {}

const withoutKey = (o: ShortcutOverrides, id: string): ShortcutOverrides => Object.fromEntries(Object.entries(o).filter(([k]) => k !== id))

type Notice =
  | { id: string; kind: 'conflict'; accel: string; other: string; reset?: boolean }
  | { id: string; kind: 'reserved'; label: string }
  | { id: string; kind: 'modifier' | 'unsupported' }

/** Shortcuts section of Settings: each command is changed by clicking its shortcut then typing the new one. */
export function ShortcutSettings({ info }: { info: SettingsInfo }) {
  const { t } = useTranslation()
  const overrides: ShortcutOverrides = info.settings.shortcuts ?? NO_OVERRIDES
  const [recording, setRecording] = useState<string | null>(null)
  const [notice, setNotice] = useState<Notice | null>(null)
  const [confirmReset, setConfirmReset] = useState(false)
  const layout = useRef<LayoutMap | null>(null)
  const recordingRef = useRef<string | null>(null)

  // Keyboard layout, to write the right letter (A and not Q on an AZERTY keyboard).
  useEffect(() => {
    const keyboard = (navigator as Navigator & { keyboard?: { getLayoutMap?: () => Promise<LayoutMap> } }).keyboard
    keyboard
      ?.getLayoutMap?.()
      .then((map) => (layout.current = map))
      .catch(() => {})
  }, [])

  const stop = useCallback(() => {
    if (recordingRef.current === null) return
    recordingRef.current = null
    setRecording(null)
    void window.signet.setShortcutRecording(false)
  }, [])

  const start = (id: string) => {
    setNotice(null)
    recordingRef.current = id
    setRecording(id)
    void window.signet.setShortcutRecording(true)
  }

  const save = useCallback((next: ShortcutOverrides) => {
    setNotice(null)
    void window.signet.updateSettings({ shortcuts: next })
  }, [])

  // Input: the next key becomes the shortcut (or cancels, or removes the shortcut).
  useEffect(() => {
    if (!recording) return
    const onKey = (e: KeyboardEvent) => {
      e.preventDefault()
      e.stopPropagation()
      if (e.repeat || MODIFIER_KEYS.has(e.key)) return
      const plain = !e.metaKey && !e.ctrlKey && !e.altKey && !e.shiftKey
      if (e.key === 'Escape' && plain) return stop()
      if ((e.key === 'Backspace' || e.key === 'Delete') && plain) {
        stop()
        return save({ ...overrides, [recording]: null })
      }
      const key = keyOf(e, layout.current)
      stop()
      if (!key) return setNotice({ id: recording, kind: 'unsupported' })
      const accel = accelOf(e, key)
      if (!usableAccel(accel, IS_MAC)) return setNotice({ id: recording, kind: 'modifier' })
      const current = shortcutAccels(recording, overrides, IS_MAC)
      if (current.length === 1 && accelKey(current[0], IS_MAC) === accelKey(accel, IS_MAC)) return
      // The original primary re-typed: nothing to change (do not lose the extra shortcuts).
      const main = shortcutDef(recording) && defaultAccel(shortcutDef(recording)!, IS_MAC)
      if (!Object.hasOwn(overrides, recording) && main && accelKey(main, IS_MAC) === accelKey(accel, IS_MAC)) return
      const conflict = shortcutConflict(accel, recording, overrides, IS_MAC)
      if (conflict && 'reserved' in conflict) return setNotice({ id: recording, kind: 'reserved', label: conflict.reserved })
      if (conflict) return setNotice({ id: recording, kind: 'conflict', accel, other: conflict.id })
      save({ ...overrides, [recording]: accel })
    }
    // Click elsewhere or window left: input stops (menu shortcuts come back).
    const onBlur = () => stop()
    const onPointer = (e: PointerEvent) => {
      if (!(e.target instanceof Element) || !e.target.closest(`[data-recording="${recording}"]`)) stop()
    }
    window.addEventListener('keydown', onKey, true)
    window.addEventListener('blur', onBlur)
    window.addEventListener('pointerdown', onPointer, true)
    return () => {
      window.removeEventListener('keydown', onKey, true)
      window.removeEventListener('blur', onBlur)
      window.removeEventListener('pointerdown', onPointer, true)
    }
  }, [recording, overrides, save, stop])

  // Leaving the section mid-input: give the shortcuts back.
  useEffect(() => () => stop(), [stop])

  const reset = (id: string) => {
    const next = { ...overrides }
    delete next[id]
    const def = shortcutDef(id)
    const original = def && defaultAccel(def, IS_MAC)
    // The original may be taken by another command: same warning as on input.
    const conflict = original ? shortcutConflict(original, id, next, IS_MAC) : null
    if (original && conflict && 'id' in conflict) return setNotice({ id, kind: 'conflict', accel: original, other: conflict.id, reset: true })
    save(next)
  }
  const label = (key: string) => t(key as 'settings.shortcuts.item.newTab')
  const nameOf = (id: string) => label(shortcutDef(id)?.label ?? id)
  const keysText = (accels: string[]) => accels.map((a) => accelLabel(a, IS_MAC)).join(' · ')
  const modified = Object.keys(overrides).length > 0

  return (
    <>
      <div className="st-shortcuts-head">
        <p className="st-note">{t('settings.shortcuts.intro')}</p>
        {confirmReset && modified ? (
          <InlineConfirm
            message={t('settings.shortcuts.confirmReset', { count: Object.keys(overrides).length })}
            confirmLabel={t('settings.shortcuts.resetAll')}
            onCancel={() => setConfirmReset(false)}
            onConfirm={() => {
              setConfirmReset(false)
              save({})
            }}
          />
        ) : (
          <button className="st-btn" disabled={!modified} onClick={() => setConfirmReset(true)}>
            <RotateCcw size={13} />
            {t('settings.shortcuts.resetAll')}
          </button>
        )}
      </div>
      {SHORTCUT_GROUPS.map((group) => (
        <section key={group}>
          <h3 className="st-sub">{t(`settings.shortcuts.group.${group}` as 'settings.shortcuts.group.tabs')}</h3>
          <div className="st-group">
            {SHORTCUTS.filter((s) => s.group === group).map((def) => {
              const accels = shortcutAccels(def.id, overrides, IS_MAC)
              const changed = Object.hasOwn(overrides, def.id)
              const original = defaultAccel(def, IS_MAC)
              const isRecording = recording === def.id
              const note = notice?.id === def.id ? notice : null
              return (
                <Fragment key={def.id}>
                  <div className="st-row">
                    <div className="st-label">
                      <span>{label(def.label)}</span>
                    </div>
                    {changed && (
                      <button
                        className="icon"
                        title={original ? t('settings.shortcuts.reset', { keys: accelLabel(original, IS_MAC) }) : t('settings.shortcuts.resetNone')}
                        onClick={() => reset(def.id)}
                      >
                        <RotateCcw size={14} />
                      </button>
                    )}
                    <button
                      className={`st-keys st-keys-edit${isRecording ? ' recording' : ''}${accels.length ? '' : ' none'}${changed ? ' changed' : ''}`}
                      data-recording={def.id}
                      aria-label={t('settings.shortcuts.edit', { name: label(def.label) })}
                      aria-pressed={isRecording}
                      onClick={() => (isRecording ? stop() : start(def.id))}
                    >
                      {isRecording ? t('settings.shortcuts.recording') : accels.length ? keysText(accels) : t('settings.shortcuts.none')}
                    </button>
                  </div>
                  {isRecording && (
                    <div className="st-row st-shortcut-note">
                      <span className="st-hint">{t('settings.shortcuts.recordingHint')}</span>
                    </div>
                  )}
                  {note && (
                    <div className="st-row st-shortcut-note" role="alert">
                      {note.kind === 'conflict' ? (
                        <>
                          <span className="st-hint grow">
                            <span className="st-err">{t('settings.shortcuts.conflict', { name: nameOf(note.other) })}</span>{' '}
                            {t('settings.shortcuts.takeOverHint', { name: nameOf(note.other) })}
                          </span>
                          <button className="st-btn" onClick={() => setNotice(null)}>
                            {t('common.cancel')}
                          </button>
                          <button
                            className="st-btn st-btn-accent"
                            onClick={() => {
                              const next = takeOver(overrides, def.id, note.accel, note.other, IS_MAC)
                              save(note.reset ? withoutKey(next, def.id) : next)
                            }}
                          >
                            {t('settings.shortcuts.takeOver')}
                          </button>
                        </>
                      ) : (
                        <span className="st-hint grow st-err">
                          {note.kind === 'reserved'
                            ? t('settings.shortcuts.reservedBy', { name: label(note.label) })
                            : note.kind === 'modifier'
                              ? IS_MAC
                                ? t('settings.shortcuts.needModifier')
                                : t('settings.shortcuts.needModifierOther')
                              : t('settings.shortcuts.unsupported')}
                        </span>
                      )}
                    </div>
                  )}
                </Fragment>
              )
            })}
            {FIXED_SHORTCUTS.filter((f) => f.group === group).map((f) => (
              <div key={f.label} className="st-row">
                <div className="st-label">
                  <span>{label(f.label)}</span>
                </div>
                <kbd className="st-keys fixed" title={t('settings.shortcuts.fixed')}>
                  {!IS_MAC && f.other ? f.other : platformKeys(f.keys)}
                </kbd>
              </div>
            ))}
          </div>
        </section>
      ))}
    </>
  )
}

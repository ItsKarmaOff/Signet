import { useEffect, useMemo, useRef, useState } from 'react'
import { ImagePlus } from 'lucide-react'
import { useTranslation } from 'react-i18next'
import type { OverlayPayload } from '@shared/types'
import { domainOf } from '../lib/util'

type Payload = Extract<OverlayPayload, { mode: 'webapp' }>

const WIDTH = 320
/** macOS icon grid: 1024 px square, 824 px plate with rounded corners, light shadow. */
const SIZE = 1024
const PLATE = 824
const RADIUS = 185

/** True if the image's corners are opaque (full square, to stretch over the whole plate rather than center in it). */
function opaqueCorners(img: HTMLImageElement): boolean {
  const c = document.createElement('canvas')
  c.width = c.height = 32
  const ctx = c.getContext('2d', { willReadFrequently: true })
  if (!ctx) return false
  ctx.drawImage(img, 0, 0, 32, 32)
  try {
    const at = (x: number, y: number) => ctx.getImageData(x, y, 1, 1).data[3]
    return [at(1, 1), at(30, 1), at(1, 30), at(30, 30)].every((a) => a > 240)
  } catch {
    return false
  }
}

/**
 * How the image is framed in the icon: `fill` the whole rounded plate, `fit` placed at 60% on a white plate,
 * `raw` the image as is over the whole square (an icon already drawn for the Mac).
 */
type Fit = 'fill' | 'fit' | 'raw'
const FITS: Fit[] = ['fill', 'fit', 'raw']

/** Image chosen by the user: 15 MB at most (it is downscaled to 1024 px anyway). */
const MAX_CUSTOM_BYTES = 15_000_000

/** Suggested framing: plate filled for a maskable icon or a crisp opaque square, otherwise placed on a white background. */
function autoFit(img: HTMLImageElement, maskable: boolean): Fit {
  const natural = Math.min(img.naturalWidth || 0, img.naturalHeight || 0)
  return maskable || (natural >= 120 && opaqueCorners(img)) ? 'fill' : 'fit'
}

/**
 * App icon as a 1024 px PNG: the image on a rounded plate according to `fit`, or, without an image, the name's initial
 * on the space's tint.
 */
function compose(img: HTMLImageElement | null, fit: Fit, name: string, hue: number): string {
  const c = document.createElement('canvas')
  c.width = c.height = SIZE
  const ctx = c.getContext('2d')
  if (!ctx) return ''
  ctx.imageSmoothingEnabled = true
  ctx.imageSmoothingQuality = 'high'
  const w0 = img?.naturalWidth || PLATE
  const h0 = img?.naturalHeight || PLATE
  if (img && fit === 'raw') {
    // As is: contained in the whole square, no plate or shadow.
    const ratio = Math.min(SIZE / w0, SIZE / h0)
    ctx.drawImage(img, (SIZE - w0 * ratio) / 2, (SIZE - h0 * ratio) / 2, w0 * ratio, h0 * ratio)
    return toPng(c, img, name, hue)
  }
  const o = (SIZE - PLATE) / 2
  const plate = () => {
    ctx.beginPath()
    ctx.roundRect(o, o, PLATE, PLATE, RADIUS)
  }
  // Drop shadow of the plate, like system icons.
  ctx.save()
  ctx.shadowColor = 'rgba(0, 0, 0, 0.3)'
  ctx.shadowBlur = 28
  ctx.shadowOffsetY = 12
  plate()
  ctx.fillStyle = img ? '#ffffff' : `hsl(${hue} 50% 48%)`
  ctx.fill()
  ctx.restore()

  ctx.save()
  plate()
  ctx.clip()
  if (img && fit === 'fill') {
    // Covers the plate while keeping proportions.
    const ratio = Math.max(PLATE / w0, PLATE / h0)
    ctx.drawImage(img, o + (PLATE - w0 * ratio) / 2, o + (PLATE - h0 * ratio) / 2, w0 * ratio, h0 * ratio)
  } else if (img) {
    const box = PLATE * 0.6
    const ratio = Math.min(box / w0, box / h0)
    ctx.drawImage(img, (SIZE - w0 * ratio) / 2, (SIZE - h0 * ratio) / 2, w0 * ratio, h0 * ratio)
  } else {
    const shade = ctx.createLinearGradient(0, o, 0, o + PLATE)
    shade.addColorStop(0, 'rgba(255, 255, 255, 0.18)')
    shade.addColorStop(1, 'rgba(0, 0, 0, 0.12)')
    ctx.fillStyle = shade
    ctx.fillRect(o, o, PLATE, PLATE)
    const letter = Array.from(name.trim())[0]?.toLocaleUpperCase() ?? '?'
    ctx.fillStyle = '#ffffff'
    ctx.font = '600 440px -apple-system, "SF Pro Display", system-ui, sans-serif'
    ctx.textAlign = 'center'
    ctx.textBaseline = 'middle'
    ctx.fillText(letter, SIZE / 2, SIZE / 2 + 16)
  }
  ctx.restore()
  // Subtle hairline: a white plate stays visible on a light background.
  if (img && fit === 'fit') {
    plate()
    ctx.lineWidth = 3
    ctx.strokeStyle = 'rgba(0, 0, 0, 0.08)'
    ctx.stroke()
  }
  return toPng(c, img, name, hue)
}

function toPng(c: HTMLCanvasElement, img: HTMLImageElement | null, name: string, hue: number): string {
  try {
    return c.toDataURL('image/png')
  } catch {
    // Image that "taints" the canvas (external SVG…): fall back to the initial.
    return img ? compose(null, 'fit', name, hue) : ''
  }
}

/** Loads an image (data URL); null if it is unreadable. */
function loadImage(src: string): Promise<HTMLImageElement | null> {
  return new Promise((resolve) => {
    const image = new Image()
    image.onload = () => resolve(image.naturalWidth > 0 ? image : null)
    image.onerror = () => resolve(null)
    image.src = src
  })
}

/** Offer to install the site as an app (route #overlay, `webapp` mode), at the top right of the page. */
export function WebAppInstall({ payload }: { payload: Payload }) {
  const { t } = useTranslation()
  const { token, url, icon, maskable, hue, card } = payload
  const [name, setName] = useState(payload.name)
  const [site, setSite] = useState<HTMLImageElement | null>(null)
  /** Image chosen by the user (file, drag and drop or paste), which replaces the site's icon. */
  const [custom, setCustom] = useState<HTMLImageElement | null>(null)
  const [fit, setFit] = useState<Fit>('fit')
  const [bad, setBad] = useState(false)
  const [dragging, setDragging] = useState(false)
  const [busy, setBusy] = useState(false)
  const fileRef = useRef<HTMLInputElement>(null)

  useEffect(() => {
    if (!icon) return
    let alive = true
    void loadImage(icon).then((image) => {
      if (!alive || !image) return
      setSite(image)
      setFit(autoFit(image, maskable))
    })
    return () => {
      alive = false
    }
  }, [icon, maskable])

  const takeFile = (file: File | null | undefined) => {
    if (!file) return
    if (!file.type.startsWith('image/') || file.size > MAX_CUSTOM_BYTES) return setBad(true)
    const reader = new FileReader()
    reader.onload = () =>
      void loadImage(String(reader.result)).then((image) => {
        setBad(!image)
        if (!image) return
        setCustom(image)
        setFit(autoFit(image, false))
      })
    reader.onerror = () => setBad(true)
    reader.readAsDataURL(file)
  }

  const resetToSite = () => {
    setCustom(null)
    setBad(false)
    setFit(site ? autoFit(site, maskable) : 'fit')
  }

  const img = custom ?? site
  // Without an image, the initial follows the typed name.
  const letterName = img ? '' : name
  const preview = useMemo(() => compose(img, fit, letterName || payload.name, hue), [img, fit, letterName, payload.name, hue])

  const install = () => {
    if (busy) return
    setBusy(true)
    const final = name.trim() || payload.name
    void window.signet.installWebApp(token, final, compose(img, fit, final, hue))
  }

  const left = Math.max(8, Math.min(card.x + card.width - WIDTH - 12, window.innerWidth - WIDTH - 8))
  const top = card.y + 12
  return (
    <div className="site-layer" onMouseDown={() => void window.signet.closeOverlay()}>
      <div
        className={`site-panel pw-prompt wa-install${dragging ? ' dropping' : ''}`}
        style={{ left, top, width: WIDTH, transformOrigin: 'top right' }}
        onMouseDown={(e) => e.stopPropagation()}
        // Drag an image onto the card, or paste it (⌘V): it becomes the icon.
        onDragOver={(e) => {
          if (![...e.dataTransfer.types].includes('Files')) return
          e.preventDefault()
          e.dataTransfer.dropEffect = 'copy'
          setDragging(true)
        }}
        onDragLeave={(e) => {
          if (!e.currentTarget.contains(e.relatedTarget as Node | null)) setDragging(false)
        }}
        onDrop={(e) => {
          e.preventDefault()
          setDragging(false)
          takeFile(e.dataTransfer.files[0])
        }}
        onPaste={(e) => {
          const file = [...e.clipboardData.files].find((f) => f.type.startsWith('image/'))
          if (!file) return
          e.preventDefault()
          takeFile(file)
        }}
      >
        <div className="site-head">
          <div className="site-id">
            <strong>{t('webapp.installTitle')}</strong>
            <span className="site-conn">{domainOf(url)}</span>
          </div>
        </div>
        <form
          className="wa-install-body"
          onSubmit={(e) => {
            e.preventDefault()
            install()
          }}
        >
          <button
            type="button"
            className="wa-install-pick"
            title={t('webapp.iconHint')}
            aria-label={t('webapp.chooseImage')}
            onClick={() => fileRef.current?.click()}
          >
            {preview && <img className="wa-install-icon" src={preview} alt="" draggable={false} />}
            <span className="wa-install-badge" aria-hidden>
              <ImagePlus size={12} strokeWidth={2.2} />
            </span>
          </button>
          <label className="wa-install-name">
            <span>{t('webapp.name')}</span>
            <input className="st-input" value={name} maxLength={60} autoFocus spellCheck={false} onChange={(e) => setName(e.target.value)} />
          </label>
          <input
            ref={fileRef}
            type="file"
            accept="image/png,image/jpeg,image/webp,image/gif,image/svg+xml,image/x-icon,image/bmp"
            hidden
            onChange={(e) => {
              takeFile(e.target.files?.[0])
              e.target.value = '' // the same file can be chosen again
            }}
          />
        </form>
        <div className="wa-install-img">
          {img && (
            <div className="seg-ctl" role="radiogroup" aria-label={t('webapp.fitLabel')}>
              {FITS.map((f) => (
                <button key={f} type="button" role="radio" aria-checked={fit === f} className={fit === f ? 'on' : ''} onClick={() => setFit(f)}>
                  {t(`webapp.fit.${f}`)}
                </button>
              ))}
            </div>
          )}
          <div className="wa-install-img-actions">
            <button type="button" className="site-btn-s" onClick={() => fileRef.current?.click()}>
              {t('webapp.chooseImage')}
            </button>
            {custom && (
              <button type="button" className="site-btn-s" onClick={resetToSite}>
                {site ? t('webapp.siteIcon') : t('webapp.noImage')}
              </button>
            )}
            {bad && (
              <span className="wa-install-err" role="alert">
                {t('webapp.badImage')}
              </span>
            )}
          </div>
        </div>
        <p className="wa-install-hint">{t('webapp.installHint')}</p>
        <div className="site-foot pw-actions">
          <span className="pw-grow" />
          <button className="site-btn-s" onClick={() => void window.signet.closeOverlay()}>
            {t('common.cancel')}
          </button>
          <button className="site-btn-s pw-primary" disabled={busy || !name.trim()} onClick={install}>
            {t('webapp.install')}
          </button>
        </div>
      </div>
    </div>
  )
}

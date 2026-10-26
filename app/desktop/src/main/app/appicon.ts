import { app, nativeImage, type NativeImage } from 'electron'
import { join } from 'node:path'
import { APP_ICONS } from '@shared/types'
import type { Browser } from '../browser/browser'
import { integrateAppImage, setWindowIcon } from './linux'

export function isAppIcon(id: string): boolean {
  return APP_ICONS.some((i) => i.id === id)
}

// In development, we start from the compiled code (out/main): the app folder depends on how Electron is launched.
function iconsDir(): string {
  return app.isPackaged ? join(process.resourcesPath, 'icons') : join(__dirname, '../../resources/icons')
}

function load(id: string): NativeImage {
  const path = join(iconsDir(), `${isAppIcon(id) ? id : 'aurore'}.png`)
  const image = nativeImage.createFromPath(path)
  if (image.isEmpty()) console.warn(`Icon not found: ${path}`)
  return image
}

const previews = new Map<string, string>()

/** 128 px preview as a data URL (sharp on Retina), computed once per variant. */
export function iconPreview(id: string): string {
  let url = previews.get(id)
  if (!url) {
    url = load(id).resize({ width: 128, height: 128, quality: 'best' }).toDataURL()
    previews.set(id, url)
  }
  return url
}

/** Precomputes the previews in the background (macOS thumbnails, off the main thread); `iconPreview` keeps the synchronous computation as a fallback. */
export async function warmIconPreviews(): Promise<void> {
  for (const { id } of APP_ICONS) {
    if (previews.has(id)) continue
    try {
      const thumb = await nativeImage.createThumbnailFromPath(join(iconsDir(), `${id}.png`), { width: 128, height: 128 })
      const { width, height } = thumb.getSize()
      // Expected 128 px square; otherwise we leave the synchronous fallback.
      if (!thumb.isEmpty() && width === 128 && height === 128 && !previews.has(id)) previews.set(id, thumb.toDataURL())
    } catch {
      /* synchronous fallback on demand */
    }
  }
}

/** Applies the variant to the Dock icon (Mac), or to the windows and the AppImage shortcut (Linux). */
export function applyAppIcon(id: string, changed = false): void {
  const image = load(id)
  if (image.isEmpty()) return
  app.dock?.setIcon(image)
  setWindowIcon(image)
  void integrateAppImage(image, changed)
}

export function setAppIcon(b: Browser, id: string): void {
  if (!isAppIcon(id)) return
  b.store.state.settings.appIcon = id
  applyAppIcon(id, true)
  b.broadcast.changed()
}

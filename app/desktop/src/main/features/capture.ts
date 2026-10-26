import { app, BrowserWindow, dialog, nativeImage, type NativeImage, type WebContents } from 'electron'
import { writeFile } from 'node:fs/promises'
import { join } from 'node:path'
import type { CaptureTarget, Rect } from '@shared/types'
import type { Browser } from '../browser/browser'
import { t } from '../app/i18n'
import { safeFileBase } from './pagetools'

/** Longest side of a full-page capture (px): beyond it, the engine fails or the image becomes huge. */
const MAX_SIDE = 16384

/** File name of a capture: "Capture example.com 2026-10-24 at 16.42.05.png". */
function fileName(url: string): string {
  let host = ''
  try {
    host = new URL(url).hostname.replace(/^www\./, '')
  } catch {
    // address without host
  }
  const d = new Date()
  const pad = (n: number) => String(n).padStart(2, '0')
  const date = `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`
  const time = `${pad(d.getHours())}.${pad(d.getMinutes())}.${pad(d.getSeconds())}`
  return `${safeFileBase(t('capture.fileName', { site: host || 'Signet', date, time }))}.png`
}

/** Screenshot tool: an area of the page, the visible part or the whole page, to copy or save. */
export class Capture {
  constructor(private b: Browser) {}

  private contents(tabId: string | null): WebContents | null {
    const wc = tabId ? this.b.views.viewOf(tabId)?.webContents : undefined
    return wc && !wc.isDestroyed() ? wc : null
  }

  /**
   * Starts the tool on the tab. In the main window, the overlay offers area, visible part or whole page;
   * elsewhere (Little Signet, private window), the visible part is offered for saving right away.
   */
  start(tabId: string | null): void {
    const wc = this.contents(tabId)
    if (!wc || !tabId) return
    if (this.b.overlay.openCapture(tabId)) return
    void this.grab(wc, { kind: 'visible' }).then((img) => img && !img.isEmpty() && this.save(wc, img))
  }

  /**
   * Takes the capture requested by the overlay. `copy` returns the image as PNG (data URL): the interface puts it in the
   * clipboard (the main process clipboard API no longer accepts images). `save` closes the overlay then asks where to save.
   */
  async take(tabId: string, target: CaptureTarget, action: 'copy' | 'save' | 'easel'): Promise<{ ok: boolean; png?: string }> {
    const wc = this.contents(tabId)
    if (!wc) return { ok: false }
    const img = await this.grab(wc, target)
    if (!img || img.isEmpty()) return { ok: false }
    if (action === 'copy') return { ok: true, png: img.toDataURL() }
    if (action === 'easel') {
      // New Easel with the capture, reduced to 1600 px per side (the limit for images in an Easel).
      this.b.overlay.close()
      const { width, height } = img.getSize()
      const k = Math.min(1, 1600 / Math.max(width, height, 1))
      const small = k < 1 ? img.resize({ width: Math.round(width * k), height: Math.round(height * k), quality: 'good' }) : img
      this.b.notes.createEaselWithImage(small.toDataURL(), Math.round(width * k), Math.round(height * k))
      return { ok: true }
    }
    this.b.overlay.close()
    return { ok: await this.save(wc, img) }
  }

  /** Save dialog (Downloads, dated name), then writes the PNG. */
  private async save(wc: WebContents, img: NativeImage): Promise<boolean> {
    const win = BrowserWindow.fromWebContents(wc) ?? this.b.win
    const options: Electron.SaveDialogOptions = {
      title: t('capture.saveTitle'),
      defaultPath: join(app.getPath('downloads'), fileName(wc.getURL())),
      filters: [{ name: 'PNG', extensions: ['png'] }]
    }
    const { canceled, filePath } = win ? await dialog.showSaveDialog(win, options) : await dialog.showSaveDialog(options)
    if (canceled || !filePath) return false
    try {
      await writeFile(filePath, img.toPNG())
      return true
    } catch (error) {
      console.error('Capture :', error)
      const message: Electron.MessageBoxOptions = { type: 'warning', message: t('capture.saveFailed') }
      void (win ? dialog.showMessageBox(win, message) : dialog.showMessageBox(message))
      return false
    }
  }

  private async grab(wc: WebContents, target: CaptureTarget): Promise<NativeImage | null> {
    try {
      if (target.kind === 'full') return await this.fullPage(wc)
      if (target.kind === 'region') {
        const r = this.clampRect(wc, target.rect)
        return r ? await wc.capturePage(r) : null
      }
      // Reduced responsive mode: the image may exceed the view, so it is cropped to its bounds.
      const bounds = this.b.views.viewOf(this.b.views.tabIdOfContents(wc.id) ?? '')?.getBounds()
      return bounds && bounds.width >= 2 && bounds.height >= 2
        ? await wc.capturePage({ x: 0, y: 0, width: bounds.width, height: bounds.height })
        : await wc.capturePage()
    } catch (error) {
      console.error('Capture :', error)
      return null
    }
  }

  /** Requested area, brought within the view bounds (view coordinates, in points). */
  private clampRect(wc: WebContents, rect: Rect): Rect | null {
    const view = this.b.views.viewOf(this.b.views.tabIdOfContents(wc.id) ?? '')
    const bounds = view?.getBounds()
    const w = bounds?.width ?? Infinity
    const h = bounds?.height ?? Infinity
    const x = Math.max(0, Math.round(rect.x))
    const y = Math.max(0, Math.round(rect.y))
    const width = Math.min(Math.round(rect.width), w - x)
    const height = Math.min(Math.round(rect.height), h - y)
    return width >= 2 && height >= 2 ? { x, y, width, height } : null
  }

  /** Whole page, beyond the visible part (debugging protocol: already attached for site languages). */
  private async fullPage(wc: WebContents): Promise<NativeImage | null> {
    const dbg = wc.debugger
    if (!dbg.isAttached()) dbg.attach('1.3')
    const metrics = (await dbg.sendCommand('Page.getLayoutMetrics')) as { cssContentSize?: { width: number; height: number } }
    const size = metrics.cssContentSize
    if (!size) return null
    const width = Math.ceil(Math.min(size.width, MAX_SIDE))
    const height = Math.ceil(Math.min(size.height, MAX_SIDE))
    // Actual density of the page (emulated or zoomed), not the screen's.
    const real = (await dbg.sendCommand('Runtime.evaluate', { expression: 'window.devicePixelRatio', returnByValue: true }).catch(() => null)) as {
      result?: { value?: unknown }
    } | null
    const value = real?.result?.value
    const dpr = typeof value === 'number' && value > 0 ? value : 1
    // The engine already applies the screen density (`dpr`): we only shrink if the image would exceed the maximum size.
    const scale = Math.max(0.1, Math.min(1, MAX_SIDE / (Math.max(width, height) * dpr)))
    const shot = (await dbg.sendCommand('Page.captureScreenshot', {
      format: 'png',
      captureBeyondViewport: true,
      clip: { x: 0, y: 0, width, height, scale }
    })) as { data?: string }
    return shot.data ? nativeImage.createFromBuffer(Buffer.from(shot.data, 'base64')) : null
  }
}

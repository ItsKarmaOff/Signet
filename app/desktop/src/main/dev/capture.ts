import { app, BrowserWindow, nativeImage } from 'electron'
import { writeFileSync } from 'node:fs'
import { isAbsolute } from 'node:path'
import type { Pane, PopoverRequest, Rect } from '@shared/types'
import type { Browser } from '../browser/browser'
import { t } from '../app/i18n'
import { seedHistory } from './seed'
import { applyMainOptions, runUiProbes } from './probes'

/** Copies each web view's capture into the window's BGRA bitmap (12 px rounded corners respected). */
async function composePanes(b: Browser, wc: Electron.WebContents, dest: Buffer, destWidth: number, contentWidth: number): Promise<void> {
  const scale = destWidth / contentWidth
  const panes = (await wc.executeJavaScript('window.signet.getState().then((s) => s.panes)')) as Pane[]
  const views = panes.flatMap((p) => [
    { view: b.views.viewOf(p.tabId), rect: p.rect },
    // Built-in developer tools: their view sits next to the page.
    ...(p.devtools ? [{ view: b.devtools.viewOf(p.tabId), rect: p.devtools.rect }] : [])
  ])
  for (const { view, rect } of views) {
    if (!view || view.webContents.isDestroyed()) continue
    let shot = await view.webContents.capturePage()
    if (shot.isEmpty()) continue
    // Responsive mode: the capture covers the emulated device size, with the shrunken page drawn at the top left;
    // the screen only shows the part the size of the view.
    const want = { width: Math.round(rect.width * scale), height: Math.round(rect.height * scale) }
    const got = shot.getSize()
    if (got.width > want.width + 2 || got.height > want.height + 2)
      shot = shot.crop({ x: 0, y: 0, width: Math.min(want.width, got.width), height: Math.min(want.height, got.height) })
    const src = shot.toBitmap()
    const { width: sw, height: sh } = shot.getSize()
    const ox = Math.round(rect.x * scale)
    const oy = Math.round(rect.y * scale)
    const r = 12 * scale
    for (let y = 0; y < sh; y++) {
      for (let x = 0; x < sw; x++) {
        // Outside the rounded corners only.
        const cx = x < r ? r - x : x >= sw - r ? x - (sw - r) + 1 : 0
        const cy = y < r ? r - y : y >= sh - r ? y - (sh - r) + 1 : 0
        if (cx > 0 && cy > 0 && cx * cx + cy * cy > r * r) continue
        const d = ((oy + y) * destWidth + ox + x) * 4
        const o = (y * sw + x) * 4
        if (d + 3 >= dest.length || ox + x >= destWidth) continue
        dest[d] = src[o]
        dest[d + 1] = src[o + 1]
        dest[d + 2] = src[o + 2]
        dest[d + 3] = 255
      }
    }
  }
}

/** Overlay view of the main window (#overlay route), if it is placed. */
function overlayContents(b: Browser): Electron.WebContents | undefined {
  return b.win?.contentView.children
    .map((v) => (v as unknown as { webContents?: Electron.WebContents }).webContents)
    .find((c) => !!c && !c.isDestroyed() && c.getURL().endsWith('#overlay'))
}

/** Places the capture of a transparent view (alpha respected) onto the window's BGRA bitmap. */
function blendView(dest: Buffer, destWidth: number, shot: Electron.NativeImage): void {
  const src = shot.toBitmap()
  const { width: sw, height: sh } = shot.getSize()
  for (let y = 0; y < sh; y++) {
    for (let x = 0; x < sw && x < destWidth; x++) {
      const o = (y * sw + x) * 4
      const a = src[o + 3] / 255
      if (a === 0) continue
      const d = (y * destWidth + x) * 4
      if (d + 3 >= dest.length) continue
      for (let k = 0; k < 3; k++) dest[d + k] = Math.round(src[o + k] + dest[d + k] * (1 - a))
    }
  }
}

const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms))

/** SIGNET_POPOVER=spaces|folder|space-form|downloads: opens the tab row popover under its pill (captures only). */
async function openDevPopover(b: Browser, wc: Electron.WebContents, kind: string): Promise<void> {
  const selector = kind === 'folder' ? '[data-folder="dossier-bleu"]' : kind === 'downloads' ? '.dl-btn' : '.ts-space'
  const anchor = (await wc.executeJavaScript(
    `(() => { const r = document.querySelector('${selector}')?.getBoundingClientRect(); return r ? { x: r.left, y: r.top, width: r.width, height: r.height } : null })()`
  )) as Rect | null
  if (!anchor) return console.log('POPOVER ancre introuvable', selector)
  const popover: PopoverRequest =
    kind === 'folder'
      ? { kind: 'folder', folderId: 'dossier-bleu' }
      : kind === 'downloads'
        ? { kind: 'downloads' }
        : kind === 'space-form'
          ? { kind: 'space-form', spaceId: null }
          : { kind: 'spaces' }
  b.overlay.openPopover(popover, anchor)
  await sleep(700)
}

/**
 * Development hook: SIGNET_CAPTURE=<target>:<path.png> (targets: main, settings, settings:<section>, library).
 * Opens the targeted window, waits after loading, saves a capture then quits. Options (SIGNET_COMPACT, SIGNET_POPOVER…)
 * are described in docs/DEVELOPMENT.md. No effect in the packaged app.
 */
export function devCapture(b: Browser): void {
  const spec = process.env.SIGNET_CAPTURE
  if (app.isPackaged || !spec) return
  const m = /^(main|settings|library)(?::([a-z]{2,}))?:(.+\.png)$/.exec(spec)
  if (!m || !isAbsolute(m[3])) return
  const [, target, section, file] = m

  let win: BrowserWindow | null
  if (target === 'settings') {
    b.settings.open()
    win = BrowserWindow.getAllWindows().find((w) => w.getTitle() === t('window.settings')) ?? null
  } else if (target === 'library') {
    seedHistory(b)
    b.library.open()
    win = BrowserWindow.getAllWindows().find((w) => w.getTitle() === t('window.library')) ?? null
  } else {
    win = b.win
  }
  if (!win) return app.quit()
  const wc = win.webContents
  if (target === 'main') applyMainOptions(b)

  const shoot = async (): Promise<void> => {
    try {
      await runUiProbes(b)
      // capturePage ignores vibrancy: we simulate a sidebar background (capture only).
      const fill = target === 'settings' ? 'html.route-settings' : target === 'library' ? 'html.route-library' : 'html.route-main'
      await wc.insertCSS(`${fill} { background: #d9d9de !important } @media (prefers-color-scheme: dark) { ${fill} { background: #2a2a2f !important } }`)
      // SIGNET_SPACE_FORM=1: space form open; SIGNET_POPOVER: tab row popover; SIGNET_SWIPE=<-1..1>: frozen swipe between spaces.
      if (target === 'main' && process.env.SIGNET_SPACE_FORM === '1') {
        b.spaces.edit(b.space.id)
        await sleep(500)
      }
      if (target === 'main' && process.env.SIGNET_POPOVER) await openDevPopover(b, wc, process.env.SIGNET_POPOVER)
      if (target === 'main' && process.env.SIGNET_SWIPE) {
        await wc.executeJavaScript(
          `(() => { const a = document.querySelector('aside.sidebar'); const w = document.querySelector('.space-clip').clientWidth; const f = ${Number(process.env.SIGNET_SWIPE)}; a.setAttribute('data-swiping', ''); a.querySelector('.space-track').style.setProperty('--swipe-x', f * w + 'px'); a.style.setProperty('--swipe-pos', String(0 - f)) })()`
        )
        await sleep(200)
      }
      // SIGNET_UIJS=<script>: (async) script run in the interface before the capture; its result is printed.
      if (process.env.SIGNET_UIJS) console.log('UIJS', JSON.stringify(await wc.executeJavaScript(process.env.SIGNET_UIJS)))
      if (section) {
        await wc.executeJavaScript(`document.querySelector('[data-section="${section}"]')?.click()`)
        await sleep(400)
      }
      await captureExtras(b, file)
      const img = await win!.capturePage()
      // The window is translucent: we flatten the alpha so the capture is readable.
      const bmp = img.toBitmap()
      for (let i = 3; i < bmp.length; i += 4) bmp[i] = 255
      const { width, height } = img.getSize()
      // Web views are not in the window capture: we composite their own capture into it, at the `rect` positions.
      if (target === 'main') await composePanes(b, wc, bmp, width, win!.getContentBounds().width)
      // Tab row popover: its view (overlay) is placed on top of the composition.
      const popView = target === 'main' && process.env.SIGNET_POPOVER ? overlayContents(b) : undefined
      if (popView) blendView(bmp, width, await popView.capturePage())
      writeFileSync(file, nativeImage.createFromBitmap(bmp, { width, height }).toPNG())
    } finally {
      app.exit(0)
    }
  }
  // SIGNET_WAIT=<ms>: delay before capturing the main window (time to load the real pages).
  const wait = Number(process.env.SIGNET_WAIT) || 3500
  const go = (): void => {
    setTimeout(() => void shoot(), target === 'main' ? wait : 1500)
  }
  if (wc.isLoading()) wc.once('did-finish-load', go)
  else go()
}

/** Side captures, next to the requested file: site panel (`-site`), detached tools (`-devtools`, `-devtools-view`), floating bar (`-float`). */
async function captureExtras(b: Browser, file: string): Promise<void> {
  const sibling = (suffix: string) => file.replace(/\.png$/, `-${suffix}.png`)
  if (process.env.SIGNET_SITE === '1') {
    const ov = overlayContents(b)
    if (ov) writeFileSync(sibling('site'), (await ov.capturePage()).toPNG())
  }
  const dtWin = BrowserWindow.getAllWindows().find((w) => b.devtools.tabIdOf(w))
  if (dtWin) {
    writeFileSync(sibling('devtools'), (await dtWin.capturePage()).toPNG())
    const dtId = b.devtools.tabIdOf(dtWin)
    const dtView = dtId ? b.devtools.viewOf(dtId) : undefined
    if (dtView) writeFileSync(sibling('devtools-view'), (await dtView.webContents.capturePage()).toPNG())
  }
  const floatView = process.env.SIGNET_FLOAT === 'toolbar' ? b.compact.view : undefined
  if (floatView) writeFileSync(sibling('float'), (await floatView.webContents.capturePage()).toPNG())
}

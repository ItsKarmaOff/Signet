import { BrowserWindow, desktopCapturer, dialog, webContents, type DesktopCapturerSource, type Session, type WebContents } from 'electron'
import { t } from '../app/i18n'

/** Maximum number of windows offered in the choice dialog. */
const MAX_WINDOWS = 8

const cut = (name: string) => (name.length > 40 ? `${name.slice(0, 39)}…` : name)

/** Source picked by the user; null if they cancel. */
async function pick(wc: WebContents, origin: string): Promise<DesktopCapturerSource | null> {
  const sources = await desktopCapturer.getSources({ types: ['screen', 'window'], thumbnailSize: { width: 0, height: 0 } })
  if (sources.length === 0) return null
  // Wayland: a single portal source, which shows its own picker.
  if (sources.length === 1 && process.platform === 'linux') return sources[0]
  const screens = sources.filter((s) => s.id.startsWith('screen:'))
  const windows = sources.filter((s) => s.id.startsWith('window:') && s.name.trim()).slice(0, MAX_WINDOWS)
  const choices = [
    ...screens.map((s, i) => ({ source: s, label: t('dialog.display.screen', { n: i + 1 }) })),
    ...windows.map((s) => ({ source: s, label: cut(s.name) }))
  ]
  if (choices.length === 0) return null
  const parent = (!wc.isDestroyed() && BrowserWindow.fromWebContents(wc)) || BrowserWindow.getFocusedWindow()
  const options = {
    type: 'question' as const,
    message: t('dialog.display.message', { origin }),
    buttons: [...choices.map((c) => c.label), t('dialog.display.cancel')],
    defaultId: choices.length,
    cancelId: choices.length
  }
  const { response } = parent ? await dialog.showMessageBox(parent, options) : await dialog.showMessageBox(options)
  return choices[response]?.source ?? null
}

/**
 * Screen sharing (`getDisplayMedia`) for a session. `allow` says whether the requesting tab is allowed
 * (`display-capture` permission, asked of the user if needed); it receives the frame's origin.
 */
export function attachDisplayMedia(ses: Session, allow: (wc: WebContents, frameUrl: string) => Promise<boolean>): void {
  ses.setDisplayMediaRequestHandler(
    (request, callback) => {
      let done = false
      const finish = (streams: Parameters<typeof callback>[0]) => {
        if (done) return
        done = true
        try {
          callback(streams)
        } catch {
          /* request already closed */
        }
      }
      void (async () => {
        const frame = request.frame
        const wc = frame ? webContents.fromFrame(frame) : null
        if (!frame || !wc || wc.isDestroyed()) return finish({})
        // Only web pages (http/https) may share the screen, never the interface or internal pages.
        if (!/^https?:\/\//.test(frame.url)) return finish({})
        if (!(await allow(wc, frame.url))) return finish({})
        const source = await pick(wc, new URL(frame.url).host)
        if (!source) return finish({})
        // System audio: Windows only, and only if the page asked for it.
        finish(request.audioRequested && process.platform === 'win32' ? { video: source, audio: 'loopback' } : { video: source })
      })().catch(() => finish({}))
    },
    // macOS 15+: system picker; the handler above serves as a fallback.
    process.platform === 'darwin' ? { useSystemPicker: true } : undefined
  )
}

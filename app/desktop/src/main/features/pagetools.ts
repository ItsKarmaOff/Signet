import { app, BrowserWindow, dialog } from 'electron'
import { writeFile } from 'node:fs/promises'
import { join } from 'node:path'
import type { Browser } from '../browser/browser'
import { t } from '../app/i18n'

/** File name valid everywhere: no `:` or brackets (IPv6), reserved Windows names suffixed, no trailing dot or space. */
export function safeFileBase(name: string): string {
  let out = name.replace(/[[\]:]/g, '-').replace(/[. ]+$/, '')
  if (/^(con|prn|aux|nul|com[1-9]|lpt[1-9])(\..*)?$/i.test(out)) out = `${out}_`
  return out
}

/** Safe file name taken from the page title (or its host), without extension. */
function baseName(title: string, url: string): string {
  let host = ''
  try {
    host = new URL(url).hostname
  } catch {
    // address without a host: keep the title
  }
  const name = (title.trim() || host || 'page')
    // eslint-disable-next-line no-control-regex -- control characters are forbidden in a file name
    .replace(/[/\\?%*:|"<>\u0000-\u001f]/g, '-')
    .replace(/\s+/g, ' ')
    .slice(0, 120)
    .trim()
  return safeFileBase(name) || 'page'
}

/** Print, export to PDF, save the page and show its source code. */
export class PageTools {
  constructor(private b: Browser) {}

  private contents(tabId: string | null) {
    const wc = tabId ? this.b.views.viewOf(tabId)?.webContents : undefined
    return wc && !wc.isDestroyed() ? wc : null
  }

  /** Window that shows the tab, to attach dialog boxes to. */
  private windowOf(wc: Electron.WebContents): BrowserWindow | null {
    return BrowserWindow.fromWebContents(wc) ?? BrowserWindow.getFocusedWindow() ?? this.b.win
  }

  /** True if the page can be printed or saved (a web page or a file, not an internal page). */
  canSave(tabId: string | null): boolean {
    const wc = this.contents(tabId)
    return !!wc && /^(https?|file|data|view-source):/i.test(wc.getURL())
  }

  /** System print dialog (which also offers "Save as PDF" on Mac). */
  print(tabId: string | null): void {
    const wc = this.contents(tabId)
    if (!wc) return
    wc.print({}, (ok, reason) => {
      if (!ok && reason && reason !== 'cancelled' && reason !== 'Print job canceled') console.error('Print:', reason)
    })
  }

  /** Exports the page to PDF (A4 or Letter format depending on language, backgrounds printed). */
  async savePdf(tabId: string | null): Promise<void> {
    const wc = this.contents(tabId)
    if (!wc) return
    const win = this.windowOf(wc)
    const options: Electron.SaveDialogOptions = {
      title: t('pageTools.savePdfTitle'),
      defaultPath: join(app.getPath('downloads'), `${baseName(wc.getTitle(), wc.getURL())}.pdf`),
      filters: [{ name: 'PDF', extensions: ['pdf'] }]
    }
    const { canceled, filePath } = win ? await dialog.showSaveDialog(win, options) : await dialog.showSaveDialog(options)
    if (canceled || !filePath || wc.isDestroyed()) return
    try {
      const letter = /^en-(us|ca)$|^es-(mx|us)$/i.test(app.getLocale())
      const data = await wc.printToPDF({ printBackground: true, pageSize: letter ? 'Letter' : 'A4' })
      await writeFile(filePath, data)
    } catch (error) {
      this.fail(win, t('pageTools.savePdfFailed'), error)
    }
  }

  /** Saves the page: complete (HTML + resources folder) or as a single file (web archive). */
  async savePage(tabId: string | null): Promise<void> {
    const wc = this.contents(tabId)
    if (!wc) return
    const win = this.windowOf(wc)
    const options: Electron.SaveDialogOptions = {
      title: t('pageTools.savePageTitle'),
      defaultPath: join(app.getPath('downloads'), `${baseName(wc.getTitle(), wc.getURL())}.html`),
      filters: [
        { name: t('pageTools.formatComplete'), extensions: ['html', 'htm'] },
        { name: t('pageTools.formatArchive'), extensions: ['mhtml', 'mht'] }
      ]
    }
    const { canceled, filePath } = win ? await dialog.showSaveDialog(win, options) : await dialog.showSaveDialog(options)
    if (canceled || !filePath || wc.isDestroyed()) return
    try {
      await wc.savePage(filePath, /\.mht(ml)?$/i.test(filePath) ? 'MHTML' : 'HTMLComplete')
    } catch (error) {
      this.fail(win, t('pageTools.savePageFailed'), error)
    }
  }

  /** Page source code in a new tab, right after this one (private window: in its own window). */
  viewSource(tabId: string | null): void {
    const wc = this.contents(tabId)
    if (!wc || !tabId) return
    const url = wc.getURL()
    if (!/^(https?|file):/i.test(url)) return
    const source = `view-source:${url}`
    if (this.b.privates.owns(tabId)) this.b.privates.openFrom(tabId, source, true)
    else if (this.b.store.detached.has(tabId)) {
      this.b.window.show()
      this.b.tabs.open(source, { activate: true })
    } else this.b.tabs.open(source, { activate: true, afterTabId: tabId })
  }

  private fail(win: BrowserWindow | null, message: string, error: unknown): void {
    console.error(message, error)
    const options: Electron.MessageBoxOptions = { type: 'warning', message, detail: error instanceof Error ? error.message : String(error) }
    void (win ? dialog.showMessageBox(win, options) : dialog.showMessageBox(options))
  }
}

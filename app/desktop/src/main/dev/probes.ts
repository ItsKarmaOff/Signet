import { app, Menu } from 'electron'
import type { Browser } from '../browser/browser'

/**
 * Options for the main window capture (SIGNET_CAPTURE=main:…), applied before the shot:
 * SIGNET_COMPACT=1 (compact mode), SIGNET_MEDIA=1 (fake media card), SIGNET_FLOAT=toolbar (floating toolbar),
 * SIGNET_DEVELOP=<JSON DevOverrides>, SIGNET_DEVTOOLS=right|bottom|window, SIGNET_CONSOLE=1, SIGNET_OPEN=<url> (+ SIGNET_OPEN_BG=<ms>), SIGNET_SITE=1.
 */
export function applyMainOptions(b: Browser): void {
  if (process.env.SIGNET_COMPACT === '1') b.shell.setCompact(true)
  if (process.env.SIGNET_MEDIA === '1') {
    const other = Object.values(b.store.state.tabs).find((t) => t.url.includes('news.ycombinator.com'))
    if (other) {
      b.media.debug = {
        tabId: other.id,
        title: 'Nuits blanches — Épisode 12',
        artist: 'Radio Exemple',
        site: 'example.fm',
        playing: true,
        muted: false,
        canSkip: true
      }
      b.broadcast.changed()
    }
  }
  if (process.env.SIGNET_FLOAT === 'toolbar') b.compact.hover(true)

  // SIGNET_DEVELOP=<JSON DevOverrides>: development settings on the active tab, then what the page sees of them (`window.__o`,
  // and the time to first response) after SIGNET_DEVELOP_WAIT ms (4500 by default).
  const develop = process.env.SIGNET_DEVELOP
  if (develop)
    setTimeout(() => {
      const id = b.space.activeTabId
      b.develop.set(id, JSON.parse(develop))
      setTimeout(
        () => {
          const wc = id ? b.views.viewOf(id)?.webContents : undefined
          void wc
            ?.executeJavaScript('JSON.stringify([window.__o ?? null, Math.round(performance.getEntriesByType("navigation")[0]?.responseStart ?? -1)])')
            .then((r) =>
              console.log(
                'DEVELOP',
                r,
                Menu.getApplicationMenu()
                  ?.items.map((i) => i.label)
                  .join(' | ')
              )
            )
            .catch(() => {})
        },
        Number(process.env.SIGNET_DEVELOP_WAIT) || 4500
      )
    }, 1500)

  const dock = process.env.SIGNET_DEVTOOLS
  if (dock === 'right' || dock === 'bottom' || dock === 'window')
    setTimeout(() => {
      const id = b.space.activeTabId
      if (id) b.devtools.setDock(id, dock)
    }, 1000)

  // Console warnings and errors from all pages (CONSOLE prefix).
  if (process.env.SIGNET_CONSOLE === '1')
    app.on('web-contents-created', (_e, c) =>
      c.on('console-message', (e) => {
        if (e.level === 'warning' || e.level === 'error') console.log(`CONSOLE ${e.level} ${c.getURL().slice(0, 60)} ${e.message.slice(0, 300)}`)
      })
    )

  // SIGNET_OPEN_BG=<ms>: the tab is first opened in the background, then shown after this delay.
  const openUrl = process.env.SIGNET_OPEN
  const bg = Number(process.env.SIGNET_OPEN_BG) || 0
  if (openUrl) {
    const id = b.tabs.open(openUrl, { activate: !bg })
    if (bg) setTimeout(() => b.tabs.activate(id), bg)
  }
  if (process.env.SIGNET_SITE === '1') setTimeout(() => b.sitePanel.open(b.win!.webContents, { x: 400, y: 10, width: 28, height: 28 }), 2500)
}

/**
 * Probes run just before the capture: SIGNET_EVAL=<script> (in the active tab), SIGNET_INSTALL_EXT=<id[,id…]> (installs extensions
 * and prints their state), SIGNET_EXT_SET=<JSON> (settings applied before the shot: order, hiding of extensions).
 */
export async function runUiProbes(b: Browser): Promise<void> {
  if (process.env.SIGNET_EVAL) {
    const v = b.views.viewOf(b.space.activeTabId!)
    console.log('EVAL', JSON.stringify(await v?.webContents.executeJavaScript(process.env.SIGNET_EVAL)))
  }
  const extId = process.env.SIGNET_INSTALL_EXT
  if (extId) {
    for (const one of extId.split(',')) await b.extensions.devInstall(one.trim())
    await new Promise((r) => setTimeout(r, 2500)) // let the action button appear
    console.log(`EXTCHECK sessions ${b.extensions.report()}`)
    console.log(
      `EXTCHECK reglages ${JSON.stringify(b.settings.info().extensions.map((e) => ({ ...e, icon: e.icon ? `${e.icon.slice(0, 22)}…` : undefined })))}`
    )
  }
  if (process.env.SIGNET_EXT_SET) {
    b.settings.update(JSON.parse(process.env.SIGNET_EXT_SET))
    await new Promise((r) => setTimeout(r, 800))
  }
}

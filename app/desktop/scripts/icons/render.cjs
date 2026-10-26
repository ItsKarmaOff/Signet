// Launched by `electron scripts/icons/render.cjs <folder> <id>...`: renders each svg/<id>.svg to <id>.png (1024x1024).
/* eslint-disable @typescript-eslint/no-require-imports -- CommonJS script launched by Electron */
const { app, BrowserWindow } = require('electron')
const { readFileSync, writeFileSync } = require('node:fs')
const { join } = require('node:path')

const [dir, ...ids] = process.argv.slice(2)
app.dock?.hide()
app
  .whenReady()
  .then(async () => {
    const win = new BrowserWindow({
      width: 1024,
      height: 1024,
      show: false,
      frame: false,
      transparent: true,
      backgroundColor: '#00000000',
      useContentSize: true,
      webPreferences: { offscreen: false, zoomFactor: 1 }
    })
    for (const id of ids) {
      const svg = readFileSync(join(dir, 'svg', id + '.svg'), 'utf8')
      const html = `<!doctype html><html><body style="margin:0;background:transparent;overflow:hidden"><div style="width:1024px;height:1024px">${svg.replace('<svg ', '<svg width="1024" height="1024" ')}</div></body></html>`
      await win.loadURL('data:text/html;charset=utf-8,' + encodeURIComponent(html))
      await new Promise((r) => setTimeout(r, 250))
      let img = await win.webContents.capturePage({ x: 0, y: 0, width: 1024, height: 1024 })
      const { width, height } = img.getSize()
      if (width !== 1024 || height !== 1024) img = img.resize({ width: 1024, height: 1024, quality: 'best' })
      writeFileSync(join(dir, id + '.png'), img.toPNG())
    }
    app.quit()
  })
  .catch((e) => {
    console.error(e)
    app.exit(1)
  })

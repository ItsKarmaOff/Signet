// Run by `electron app-icons-render.cjs <svg folder> <output> <name>...`: renders each svg/<name>.svg to <name>.png (1024x1024).
// RGB without an alpha channel (the App Store rejects them), except the Android `*-foreground` and `*-monochrome` layers: RGBA, transparent background.
/* eslint-disable @typescript-eslint/no-require-imports -- CommonJS script run by Electron */
const { app, BrowserWindow } = require('electron')
const { readFileSync, writeFileSync } = require('node:fs')
const { join } = require('node:path')
const zlib = require('node:zlib')

const [dir, out, ...ids] = process.argv.slice(2)
const SIZE = 1024

const crcTable = Array.from({ length: 256 }, (_, n) => {
  let c = n
  for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1
  return c >>> 0
})
const crc32 = (buf) => {
  let c = 0xffffffff
  for (const b of buf) c = crcTable[(c ^ b) & 0xff] ^ (c >>> 8)
  return (c ^ 0xffffffff) >>> 0
}
const chunk = (type, data) => {
  const body = Buffer.concat([Buffer.from(type), data])
  const len = Buffer.alloc(4)
  len.writeUInt32BE(data.length)
  const crc = Buffer.alloc(4)
  crc.writeUInt32BE(crc32(body))
  return Buffer.concat([len, body, crc])
}

/** BGRA (Electron bitmap) -> 8-bit RGB PNG: the App Store rejects icons with an alpha channel. */
function encodeRgb(bgra) {
  const rows = Buffer.alloc((SIZE * 3 + 1) * SIZE)
  for (let y = 0; y < SIZE; y++) {
    const row = y * (SIZE * 3 + 1)
    rows[row] = 0
    for (let x = 0; x < SIZE; x++) {
      const s = (y * SIZE + x) * 4
      const d = row + 1 + x * 3
      rows[d] = bgra[s + 2]
      rows[d + 1] = bgra[s + 1]
      rows[d + 2] = bgra[s]
    }
  }
  const ihdr = Buffer.alloc(13)
  ihdr.writeUInt32BE(SIZE, 0)
  ihdr.writeUInt32BE(SIZE, 4)
  ihdr[8] = 8
  ihdr[9] = 2
  return Buffer.concat([
    Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]),
    chunk('IHDR', ihdr),
    chunk('IDAT', zlib.deflateSync(rows, { level: 9 })),
    chunk('IEND', Buffer.alloc(0))
  ])
}

app.dock?.hide()
app
  .whenReady()
  .then(async () => {
    const win = new BrowserWindow({
      width: SIZE,
      height: SIZE,
      show: false,
      frame: false,
      transparent: true,
      backgroundColor: '#00000000',
      useContentSize: true,
      webPreferences: { offscreen: false, zoomFactor: 1 }
    })
    for (const id of ids) {
      const svg = readFileSync(join(dir, 'svg', id + '.svg'), 'utf8')
      const html = `<!doctype html><html><body style="margin:0;background:transparent;overflow:hidden"><div style="width:${SIZE}px;height:${SIZE}px">${svg.replace('<svg ', `<svg width="${SIZE}" height="${SIZE}" `)}</div></body></html>`
      await win.loadURL('data:text/html;charset=utf-8,' + encodeURIComponent(html))
      await new Promise((r) => setTimeout(r, 250))
      let img = await win.webContents.capturePage({ x: 0, y: 0, width: SIZE, height: SIZE })
      const { width, height } = img.getSize()
      if (width !== SIZE || height !== SIZE) img = img.resize({ width: SIZE, height: SIZE, quality: 'best' })
      const layer = /-(foreground|monochrome)$/.test(id)
      writeFileSync(join(out, id + '.png'), layer ? img.toPNG() : encodeRgb(img.toBitmap()))
    }
    app.quit()
  })
  .catch((e) => {
    console.error(e)
    app.exit(1)
  })

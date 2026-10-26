import { createServer, type Server } from 'node:http'
import { existsSync, mkdtempSync, readFileSync, rmSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { dirname, join } from 'node:path'
import { test, expect } from './fixtures'

test('downloads go to the folder chosen in Settings', async ({ signetApp: a }) => {
  // File served as an attachment: the engine downloads it instead of displaying it.
  const server: Server = createServer((_req, res) => {
    res.writeHead(200, { 'content-type': 'text/plain', 'content-disposition': 'attachment; filename="note.txt"' })
    res.end('bonjour')
  })
  await new Promise<void>((r) => server.listen(0, '127.0.0.1', () => r()))
  const port = (server.address() as { port: number }).port
  const dir = mkdtempSync(join(tmpdir(), 'signet-dl-'))
  try {
    await a.signet((b, d) => b.settings.update({ downloadDir: d }), dir)
    expect(await a.signet((b) => b.store.state.settings.downloadDir)).toBe(dir)

    await a.signet((b, u) => b.tabs.open(u, { activate: true }), `http://127.0.0.1:${port}/note.txt`)
    await expect.poll(async () => (await a.snapshot()).downloads[0]?.status, { timeout: 15_000 }).toBe('completed')
    const path = (await a.snapshot()).downloads[0].path
    expect(dirname(path)).toBe(dir)
    expect(readFileSync(path, 'utf8')).toBe('bonjour')

    // Back to the system folder: the setting disappears.
    await a.signet((b) => b.settings.update({ downloadDir: '' }))
    expect(await a.signet((b) => b.store.state.settings.downloadDir)).toBeUndefined()
    expect(existsSync(path)).toBe(true)
  } finally {
    server.close()
    rmSync(dir, { recursive: true, force: true })
  }
})

// UI error log (React views): userData/logs/interface.log, to find the cause of a window left empty.
import { app } from 'electron'
import { appendFileSync, mkdirSync, renameSync, statSync } from 'node:fs'
import { join } from 'node:path'

/** Beyond this size, the log becomes `interface.log.1` (only one old one kept). */
const MAX_BYTES = 256 * 1024

const file = () => join(app.getPath('userData'), 'logs', 'interface.log')

/** Appends an error to the log (and to the console); never throws. */
export function logUiError(route: string, message: string, detail = ''): void {
  const line = `[${new Date().toISOString()}] ${route.slice(0, 40)} : ${message.slice(0, 2000)}${detail ? `\n${detail.slice(0, 8000)}` : ''}\n`
  console.error(`Interface (${route}) :`, message)
  try {
    mkdirSync(join(app.getPath('userData'), 'logs'), { recursive: true })
    try {
      if (statSync(file()).size > MAX_BYTES) renameSync(file(), `${file()}.1`)
    } catch {
      // no log yet
    }
    appendFileSync(file(), line)
  } catch {
    // the log is a convenience
  }
}

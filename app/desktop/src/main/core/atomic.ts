import { closeSync, fsyncSync, openSync, renameSync, writeFileSync } from 'node:fs'
import { open, unlink } from 'node:fs/promises'

/** Writes a temporary file and fsyncs it (synchronous). */
export function writeFileSyncDurable(path: string, data: string | Buffer, mode?: number): void {
  const fd = openSync(path, 'w', mode)
  try {
    writeFileSync(fd, data)
    fsyncSync(fd)
  } finally {
    closeSync(fd)
  }
}

async function writeFileDurable(path: string, data: string): Promise<void> {
  const fh = await open(path, 'w')
  try {
    await fh.writeFile(data)
    await fh.sync()
  } finally {
    await fh.close()
  }
}

/** Atomic write (tmp + fsync + rename), asynchronous and never in parallel; `flushSync` for shutdown. */
export class AtomicWriter {
  private last: string | null = null
  private writing = false
  private again = false
  private epoch = 0

  constructor(
    private file: string,
    /** Content to write, or `null` if there is nothing to do. */
    private build: () => string | null,
    private onFail: () => void = () => {}
  ) {}

  request(): void {
    if (this.writing) {
      this.again = true
      return
    }
    this.writing = true
    void this.pump()
  }

  /** Cancels any write in progress: it will not be renamed (e.g. file deleted). */
  cancel(): void {
    this.epoch++
    this.again = false
  }

  private async pump(): Promise<void> {
    try {
      do {
        this.again = false
        const payload = this.build()
        if (payload === null || payload === this.last) continue
        const epoch = this.epoch
        const tmp = `${this.file}.tmp`
        try {
          await writeFileDurable(tmp, payload)
          // A synchronous write (shutdown) went through in the meantime: its data is more recent.
          if (epoch !== this.epoch) await unlink(tmp).catch(() => {})
          else {
            // Synchronous rename, right after the check: no `flushSync` can slip in between.
            renameSync(tmp, this.file)
            this.last = payload
          }
        } catch {
          this.onFail() // disk unavailable: we will retry on the next change
        }
      } while (this.again)
    } finally {
      this.writing = false
    }
  }

  /** True if the file is up to date (written, identical or nothing to write); false if the write failed. */
  flushSync(): boolean {
    const payload = this.build()
    if (payload === null || payload === this.last) return true
    this.epoch++
    try {
      const tmp = `${this.file}.sync.tmp`
      writeFileSyncDurable(tmp, payload)
      renameSync(tmp, this.file)
      this.last = payload
      return true
    } catch {
      this.onFail()
      return false
    }
  }
}

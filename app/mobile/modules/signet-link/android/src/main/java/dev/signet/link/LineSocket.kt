package dev.signet.link

import java.io.IOException
import java.util.concurrent.Executors
import java.util.concurrent.atomic.AtomicBoolean
import javax.net.ssl.SSLSocket

/**
 * Open pinned socket, read line by line. `onClosed` is called only once (error null = normal close).
 * Writes go through a single thread owned by the socket: they are sent in the order JS requests them.
 */
class LineSocket(
  val id: String,
  private val socket: SSLSocket,
  private val onLine: (String) -> Unit,
  private val onClosed: (String?) -> Unit,
) {
  private val closed = AtomicBoolean(false)
  private val writer by lazy { Executors.newSingleThreadExecutor() }

  /** Blocking: runs on a read thread until closed. */
  fun readLoop() {
    try {
      val input = bufferedInput(socket)
      while (true) {
        val line = readLine(input)
        if (line == null) {
          finish(null) // the other side closed cleanly
          return
        }
        onLine(line)
      }
    } catch (e: LinkException) {
      finish(e.code)
    } catch (e: IOException) {
      finish("io")
    }
  }

  /** Queues the line for writing. `done(null)` if sent, otherwise `done(code)`. */
  fun writeLine(line: String, done: (String?) -> Unit) {
    if (closed.get()) {
      done("closed")
      return
    }
    try {
      writer.execute {
        try {
          val out = socket.outputStream
          out.write((line + "\n").toByteArray(Charsets.UTF_8))
          out.flush()
          done(null)
        } catch (e: IOException) {
          done(if (closed.get()) "closed" else "io")
          finish("io")
        }
      }
    } catch (e: Exception) {
      done("closed") // writer already stopped
    }
  }

  /** Closes the socket; `error` null for an intentional close. No effect if already closed. */
  fun finish(error: String?) {
    if (!closed.compareAndSet(false, true)) return
    closeQuietly(socket)
    writer.shutdown()
    onClosed(error)
  }
}

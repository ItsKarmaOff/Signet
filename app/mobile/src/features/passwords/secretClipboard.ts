// Copies a secret to the clipboard, cleared after 60 s. The clipboard is not read back (iOS shows "Paste from…"):
// it is cleared if nothing else has been copied by the app since. A JS timer stops when the app is in the background:
// on return, clear if the delay has passed.
import { AppState } from 'react-native'
import * as Clipboard from 'expo-clipboard'

const TTL_MS = 60_000
let copiedAt = 0
let timer: ReturnType<typeof setTimeout> | null = null

const clear = () => {
  if (timer) clearTimeout(timer)
  timer = null
  if (!copiedAt) return
  copiedAt = 0
  void Clipboard.setStringAsync('').catch(() => undefined)
}

AppState.addEventListener('change', (state) => {
  if (state === 'active' && copiedAt && Date.now() - copiedAt >= TTL_MS) clear()
})

/** Copies `value`; `secret`: cleared after 60 s. */
export async function copyText(value: string, secret = false): Promise<void> {
  await Clipboard.setStringAsync(value)
  if (timer) clearTimeout(timer)
  timer = null
  copiedAt = 0
  if (!secret) return
  copiedAt = Date.now()
  timer = setTimeout(clear, TTL_MS)
}

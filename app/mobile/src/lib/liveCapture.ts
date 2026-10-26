// Snapshot of the displayed tab, to take right before leaving it (tab or space change): the previous view is unmounted
// or hidden right after, the tab switcher thumbnail would otherwise be stale. The browser (which owns the WebView) registers here;
// other functions call `captureCurrentThumbnail()` without depending on it.
type Capture = () => Promise<boolean>

let capture: Capture | null = null

/** Called by the browser: the function that captures the displayed tab. Returns the unregister function. */
export function registerLiveCapture(fn: Capture): () => void {
  capture = fn
  return () => {
    if (capture === fn) capture = null
  }
}

/** Captures the displayed tab, never blocking longer than `maxWait` ms nor throwing. */
export async function captureCurrentThumbnail(maxWait = 250): Promise<void> {
  if (!capture) return
  await Promise.race([capture().catch(() => false), new Promise((resolve) => setTimeout(resolve, maxWait))])
}

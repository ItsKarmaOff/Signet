// Page thumbnails for the tab switcher.
//
// Who calls what: the browser (which owns the WebView) calls `captureThumbnail(tabId, ref)` right before opening
// `/tabs` and when it changes tab, with the ref of the view containing the WebView. The switcher reads `useThumbnail(tabId)`.
//
// Storage: thumbnails of normal tabs are JPEG files in the temp folder (react-native-view-shot, `tmpfile`),
// lost at next launch (the index is in memory); the app then shows the card without an image (favicon + title) until the next visit.
// Those of private tabs never touch the disk: data URI kept in memory only, cleared when the tab closes.
import type { RefObject } from 'react'
import { captureRef } from 'react-native-view-shot'
import { create } from 'zustand'
import { registerThumbnailCleaner } from '@/features/settings'
import { useStore } from '@/state'

const useThumbs = create<{ uris: Record<string, string> }>(() => ({ uris: {} }))

/** Saves a tab's thumbnail (a `file://`, `data:` or `https://` address). */
export function saveThumbnail(tabId: string, uri: string): void {
  useThumbs.setState((s) => ({ uris: { ...s.uris, [tabId]: uri } }))
}

/** Forgets a tab's thumbnail. */
export function dropThumbnail(tabId: string): void {
  useThumbs.setState((s) => {
    if (!(tabId in s.uris)) return s
    const uris = { ...s.uris }
    delete uris[tabId]
    return { uris }
  })
}

/** A tab's thumbnail, or `undefined`. */
export function useThumbnail(tabId: string): string | undefined {
  return useThumbs((s) => s.uris[tabId])
}

/**
 * Captures a view (the WebView or its container) and makes it the tab's thumbnail. Never throws:
 * a failed capture (view not yet displayed, empty page) simply leaves the old thumbnail. Returns true if it succeeded.
 */
export async function captureThumbnail(tabId: string, target: RefObject<unknown> | number | null, options: { width?: number } = {}): Promise<boolean> {
  if (target === null) return false
  const tab = useStore.getState().workspace.tabs[tabId]
  // Empty page: nothing to capture, the card keeps its "new tab" state.
  if (!tab || !tab.url) {
    dropThumbnail(tabId)
    return false
  }
  const isPrivate = useStore.getState().workspace.priv.tabIds.includes(tabId)
  try {
    const uri = await captureRef(target as never, {
      format: 'jpg',
      quality: 0.55,
      width: options.width ?? 420,
      result: isPrivate ? 'data-uri' : 'tmpfile'
    })
    saveThumbnail(tabId, uri)
    return true
  } catch {
    return false
  }
}

/** Forgets all thumbnails ("Clear data › Tab previews"). */
export function clearThumbnails(): void {
  useThumbs.setState({ uris: {} })
}
registerThumbnailCleaner(clearThumbnails)

// Cleanup: a thumbnail disappears with its tab (we keep those of tabs that "Undo" can still reopen).
useStore.subscribe((state, prev) => {
  if (state.workspace.tabs === prev.workspace.tabs && state.lastClosed === prev.lastClosed && state.lastClosedBatch === prev.lastClosedBatch) return
  const keep = new Set<string>(Object.keys(state.workspace.tabs))
  if (state.lastClosed) keep.add(state.lastClosed.tab.id)
  for (const c of state.lastClosedBatch?.tabs ?? []) keep.add(c.tab.id)
  const { uris } = useThumbs.getState()
  const gone = Object.keys(uris).filter((id) => !keep.has(id))
  if (gone.length === 0) return
  const next = { ...uris }
  for (const id of gone) delete next[id]
  useThumbs.setState({ uris: next })
})

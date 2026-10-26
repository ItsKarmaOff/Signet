// Tab hover (sidebar, top row): after a stillness delay, the main process places a preview card next to it.
// A card already displayed moves from one tab to another without delay; leaving the tabs removes it after a short grace,
// then the "hot" state lasts a while longer: the next hover shows right away (crossing a row with a gap).
import type { MouseEvent } from 'react'

const DELAY = 600
const GRACE = 140
const WARM = 500

let showTimer: ReturnType<typeof setTimeout> | null = null
let hideTimer: ReturnType<typeof setTimeout> | null = null
let shown = false
/** End of the "hot" window that follows the card's removal by the grace. */
let warmUntil = 0
/** Pointer pressed or drag in progress: no preview until the next hover. */
let blocked = false

function clear(): void {
  if (showTimer) clearTimeout(showTimer)
  if (hideTimer) clearTimeout(hideTimer)
  showTimer = hideTimer = null
}

/** Hides the preview right away (click, drag, rename, scroll). */
export function hidePreview(): void {
  clear()
  if (shown) void window.signet.hideTabPreview()
  shown = false
  warmUntil = 0
}

/** Leaving the tab row: same removal, but the next hover, if it comes quickly, does not wait. */
function leave(): void {
  const was = shown
  hidePreview()
  if (was) warmUntil = Date.now() + WARM
}

/** Hover handlers of a tab row (sidebar) or a chip (top row: card placed below). */
export function previewHandlers(tabId: string) {
  const show = (el: HTMLElement) => {
    // Not during a rename (field in the row).
    if (blocked || !el.isConnected || el.querySelector('input')) return
    const r = el.getBoundingClientRect()
    shown = true
    void window.signet.showTabPreview(tabId, { x: r.left, y: r.top, width: r.width, height: r.height }, el.classList.contains('tchip'))
  }
  return {
    onMouseEnter: (e: MouseEvent<HTMLElement>) => {
      if (e.buttons) return
      blocked = false
      const el = e.currentTarget
      clear()
      if (shown || Date.now() < warmUntil) show(el)
      else showTimer = setTimeout(() => show(el), DELAY)
    },
    onMouseLeave: () => {
      if (showTimer) clearTimeout(showTimer)
      showTimer = null
      if (hideTimer) clearTimeout(hideTimer)
      hideTimer = setTimeout(leave, GRACE)
    },
    onPointerDown: () => {
      blocked = true
      hidePreview()
    },
    onDragStartCapture: () => {
      blocked = true
      hidePreview()
    }
  }
}

// The window loses the pointer (other app, native menu): no more preview.
if (typeof window !== 'undefined') {
  window.addEventListener('blur', hidePreview)
  window.addEventListener('wheel', hidePreview, { passive: true })
}

import { SignetWeb } from '../../../modules/signet-web'

// Data clearing: history lives in the store, but tab previews (thumbnails) are a cache managed by the tabs screen.
// It registers here; without registration, the "Tab previews" row is not offered.
type Cleaner = () => void | Promise<void>

let thumbnails: Cleaner | null = null

/** To be called by the module that keeps tab thumbnails; returns an unregister function. */
export function registerThumbnailCleaner(fn: Cleaner): () => void {
  thumbnails = fn
  return () => {
    if (thumbnails === fn) thumbnails = null
  }
}

export const thumbnailCleaner = (): Cleaner | null => thumbnails

// Cookies and site data: the phone's web engine keeps them. The native module signet-web (compiled build) can clear them;
// without it (Expo Go, Android), the row stays disabled with its explanation.
/** Clears cookies, cache and site storage; `null` if the native module is not present. */
export function siteDataCleaner(): Cleaner | null {
  const native = SignetWeb
  return native ? () => native.clearWebsiteData(['cookies', 'cache', 'storage']) : null
}

/** Number of sites that keep data (no data without the module). */
export async function siteDataCount(): Promise<number | null> {
  if (!SignetWeb) return null
  try {
    return (await SignetWeb.websiteDataSummary()).records
  } catch {
    return null
  }
}

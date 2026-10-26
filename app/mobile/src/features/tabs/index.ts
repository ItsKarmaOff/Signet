// "Tab switcher" feature: grid of cards with thumbnails, swipe to close, tab menu, spaces as pages,
// private tabs locked by Face ID. Route: app/tabs.tsx (full-screen modal).
// Integration: the browser calls `captureThumbnail(tab.id, ref)` before opening /tabs (see thumbnails.ts);
// `PrivateLockOverlay` is mounted in app/_layout.tsx and in the switcher; `requestPrivateAccess()` is called before any switch to private.
export { TabsScreen } from './TabsScreen'
export { captureThumbnail, saveThumbnail, dropThumbnail, useThumbnail } from './thumbnails'
export { PrivateLockOverlay, requestPrivateAccess, unlockPrivate } from './privateLock'

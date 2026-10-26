/**
 * Unique identifier (UUID v4). Uses `crypto.randomUUID` when the platform provides it (Electron, Node, browsers),
 * otherwise falls back to `Math.random` (Hermes / React Native without polyfill): these identifiers are not secrets.
 */
export function newId(): string {
  const c = (globalThis as { crypto?: { randomUUID?: () => string } }).crypto
  if (c?.randomUUID) return c.randomUUID()
  return 'xxxxxxxx-xxxx-4xxx-yxxx-xxxxxxxxxxxx'.replace(/[xy]/g, (ch) => {
    const r = Math.floor(Math.random() * 16)
    return (ch === 'x' ? r : (r & 0x3) | 0x8).toString(16)
  })
}

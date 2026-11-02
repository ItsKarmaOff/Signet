// Development only: serve the UI under its own host name. Chromium keeps one zoom level per host (ports ignored), so with
// the Vite server on `localhost`, zooming a page on any localhost port also zoomed the sidebar, toolbar and overlays.
// `*.localhost` always resolves to this machine, and Vite accepts these host names by default.
// Imported first by src/main/index.ts, before anything reads ELECTRON_RENDERER_URL.

export const DEV_UI_HOST = 'signet-ui.localhost'

/** The Vite server address with its host replaced by `DEV_UI_HOST`; unchanged when it is not a loopback address. */
export function devUiUrl(url: string): string {
  try {
    const u = new URL(url)
    if (u.hostname !== 'localhost' && u.hostname !== '127.0.0.1' && u.hostname !== '[::1]') return url
    u.hostname = DEV_UI_HOST
    // Keep the original form (no trailing slash added when there was none).
    return url.endsWith('/') ? u.href : u.href.replace(/\/$/, '')
  } catch {
    return url
  }
}

if (process.env.ELECTRON_RENDERER_URL) process.env.ELECTRON_RENDERER_URL = devUiUrl(process.env.ELECTRON_RENDERER_URL)

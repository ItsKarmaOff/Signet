// Linux: AppImage desktop integration (shortcut, icon) and window icon.
import { app, BrowserWindow, type NativeImage } from 'electron'
import { execFile } from 'node:child_process'
import { mkdir, readFile, stat, writeFile } from 'node:fs/promises'
import { homedir } from 'node:os'
import { join } from 'node:path'

/** Shortcut name: the same as `desktopName` (package.json), which also gives the window identifier under Wayland. */
const DESKTOP_ID = 'signet'
/** Line that signs our shortcuts: a shortcut written by someone else is never replaced. */
const MARK = 'X-Signet-AppImage=true'

let windowIcon: NativeImage | null = null

/** Icon of all windows, present and future (taskbar, Alt+Tab). No effect outside Linux. */
export function setWindowIcon(image: NativeImage): void {
  if (process.platform !== 'linux' || image.isEmpty()) return
  if (!windowIcon) app.on('browser-window-created', (_e, win) => windowIcon && win.setIcon(windowIcon))
  windowIcon = image
  for (const win of BrowserWindow.getAllWindows()) win.setIcon(image)
}

/** Exec field argument, quoted (Desktop Entry specification). */
const quote = (s: string) => `"${s.replace(/[\\"`$]/g, (c) => `\\${c}`).replace(/\\/g, '\\\\')}"`

/**
 * Run from an AppImage: installs (or updates if the file moved) a shortcut in
 * ~/.local/share/applications and the icon in the hicolor theme. Without it, the dock does not associate windows with an icon,
 * cannot pin the app, and Signet cannot become the default browser.
 */
export async function integrateAppImage(icon: NativeImage, iconChanged = false): Promise<void> {
  const appImage = process.env.APPIMAGE
  if (process.platform !== 'linux' || !appImage) return
  const data = process.env.XDG_DATA_HOME || join(homedir(), '.local/share')
  const apps = join(data, 'applications')
  const file = join(apps, `${DESKTOP_ID}.desktop`)
  try {
    const old = await readFile(file, 'utf8').catch(() => null)
    if (old !== null && !old.includes(MARK)) return
    const iconDir = join(data, 'icons/hicolor/512x512/apps')
    const iconFile = join(iconDir, `${DESKTOP_ID}.png`)
    const entry = [
      '[Desktop Entry]',
      'Type=Application',
      'Name=Signet',
      'GenericName=Web Browser',
      `Exec=${quote(appImage)} --ozone-platform=x11 %U`,
      `TryExec=${appImage}`,
      // Full path rather than a theme name: some docks do not re-read the icon theme after they start.
      `Icon=${iconFile}`,
      'Terminal=false',
      'StartupNotify=true',
      'StartupWMClass=Signet',
      'Categories=Network;WebBrowser;',
      'MimeType=text/html;application/xhtml+xml;x-scheme-handler/http;x-scheme-handler/https;x-scheme-handler/signet;',
      MARK,
      ''
    ].join('\n')
    // Shortcut up to date and icon present: no need to decode, shrink and rewrite the icon on every launch.
    const hasIcon = await stat(iconFile).then(
      (st) => st.size > 0,
      () => false
    )
    if (!iconChanged && old === entry && hasIcon) return
    await mkdir(iconDir, { recursive: true })
    await writeFile(iconFile, icon.resize({ width: 512, height: 512, quality: 'best' }).toPNG())
    if (old === entry) return
    await mkdir(apps, { recursive: true })
    await writeFile(file, entry)
    // Best effort: refreshes the type associations (default browser). Absent on some systems.
    execFile('update-desktop-database', [apps], () => {})
  } catch (e) {
    console.warn(`AppImage integration failed: ${(e as Error).message}`)
  }
}

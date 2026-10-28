// Engine settings (Settings › System and Privacy): graphics acceleration, secure DNS, system proxy settings.
import { app, shell } from 'electron'
import { spawn } from 'node:child_process'
import { accessSync, constants } from 'node:fs'
import { delimiter, join } from 'node:path'
import type { SecureDnsMode, Settings } from '@shared/types'
import { DEFAULT_DNS_SERVER, isDohTemplate } from '@shared/dns'
import { startupSettings } from '../web/languages'

export const isDnsMode = (v: unknown): v is SecureDnsMode => v === 'off' || v === 'automatic' || v === 'secure'

/** Before `ready`: graphics acceleration can only be turned off before the engine starts. */
export function applyStartupEngine(): void {
  if (startupSettings()?.hardwareAcceleration === false) app.disableHardwareAcceleration()
}

/** Secure DNS for the whole app. Left to the engine's default until the user chooses. */
export function applySecureDns(settings: Settings): void {
  const mode = settings.secureDns
  if (!isDnsMode(mode)) return
  const server = isDohTemplate(settings.secureDnsServer) ? settings.secureDnsServer : DEFAULT_DNS_SERVER
  try {
    app.configureHostResolver({ secureDnsMode: mode, secureDnsServers: mode === 'secure' ? [server] : [] })
  } catch (e) {
    console.error('Secure DNS: setting refused', e)
  }
}

// ───────────────────────── System proxy settings ─────────────────────────

/** Linux: system proxy settings screens, by desktop. */
const LINUX_PROXY_TOOLS: { cmd: string; args: string[] }[] = [
  { cmd: 'gnome-control-center', args: ['network'] },
  { cmd: 'systemsettings', args: ['kcm_proxy'] },
  { cmd: 'kcmshell6', args: ['kcm_proxy'] },
  { cmd: 'kcmshell5', args: ['proxy'] }
]

function onPath(cmd: string): boolean {
  for (const dir of (process.env.PATH ?? '').split(delimiter)) {
    if (!dir) continue
    try {
      accessSync(join(dir, cmd), constants.X_OK)
      return true
    } catch {
      // not in this folder
    }
  }
  return false
}

let linuxTool: (typeof LINUX_PROXY_TOOLS)[number] | null | undefined

function proxyTool(): (typeof LINUX_PROXY_TOOLS)[number] | null {
  if (linuxTool === undefined) linuxTool = LINUX_PROXY_TOOLS.find((t) => onPath(t.cmd)) ?? null
  return linuxTool
}

/** False when no system proxy settings screen is known (some Linux desktops). */
export const hasProxySettings = (): boolean => process.platform !== 'linux' || proxyTool() !== null

export function openProxySettings(): void {
  if (process.platform === 'darwin') void shell.openExternal('x-apple.systempreferences:com.apple.Network-Settings.extension').catch(() => {})
  else if (process.platform === 'win32') void shell.openExternal('ms-settings:network-proxy').catch(() => {})
  else {
    const tool = proxyTool()
    if (!tool) return
    try {
      spawn(tool.cmd, tool.args, { detached: true, stdio: 'ignore' })
        .on('error', () => {})
        .unref()
    } catch (e) {
      console.error('Proxy settings: cannot open', e)
    }
  }
}

/** Relaunch to apply graphics acceleration. Packaged app only: in development it would detach from electron-vite. */
export function relaunchApp(): void {
  if (!app.isPackaged) return
  app.relaunch()
  app.quit()
}

import { execFile } from 'node:child_process'
import { hostname, networkInterfaces } from 'node:os'
import type { LinkPlatform } from '@signet/shared/link/protocol'

export function desktopPlatform(platform: NodeJS.Platform = process.platform): Extract<LinkPlatform, 'macos' | 'windows' | 'linux'> {
  return platform === 'darwin' ? 'macos' : platform === 'win32' ? 'windows' : 'linux'
}

/** `MacBook-de-Christophe.local` → `MacBook-de-Christophe`; no control characters, 64 characters at most. */
export function cleanHostName(raw: string): string {
  const name = [...raw]
    .filter((c) => c.charCodeAt(0) > 31 && c.charCodeAt(0) !== 127)
    .join('')
    .trim()
    .replace(/\.(local|lan|home|localdomain)$/i, '')
  return name.slice(0, 64).trim() || 'Signet'
}

/** Computer name: on Mac the one from System Settings (`scutil`), elsewhere the hostname. */
export function computerName(): Promise<string> {
  if (process.platform !== 'darwin') return Promise.resolve(cleanHostName(hostname()))
  return new Promise((resolve) => {
    execFile('/usr/sbin/scutil', ['--get', 'ComputerName'], { timeout: 2000 }, (err, stdout) =>
      resolve(cleanHostName(err || !stdout.trim() ? hostname() : stdout))
    )
  })
}

/** Virtual interfaces (virtual machines, containers, VPN, AirDrop…): a phone cannot reach them. */
const VIRTUAL =
  /^(lo|utun|awdl|llw|anpi|ap\d|bridge|vmnet|vboxnet|docker|br-|veth|virbr|lxc|lxd|cni|flannel|tun|tap|zt|tailscale|wg|ham|vethernet|virtualbox|vmware|hyper-v|bluetooth)/i
/** Real Wi-Fi or Ethernet interfaces, tried first. */
const PHYSICAL = /^(en|eth|wlan|wl|wi-?fi|ethernet|eno|ens|enp|wlp)/i

type Interfaces = ReturnType<typeof networkInterfaces>

/**
 * Addresses of this computer that the phone can reach, put in the QR code: IPv4 of real interfaces first, without
 * virtual networks (Docker, OrbStack, VPN…) or link-local addresses. Three at most: the QR code stays sparse. After that, the
 * phone finds the computer through mDNS. IPv6 only if there is no IPv4 at all.
 */
export function lanAddresses(interfaces: Interfaces = networkInterfaces()): string[] {
  const v4: { address: string; physical: boolean }[] = []
  const v6: string[] = []
  for (const [name, list] of Object.entries(interfaces)) {
    if (VIRTUAL.test(name)) continue
    for (const a of list ?? []) {
      if (a.internal) continue
      if (a.family === 'IPv4' && !a.address.startsWith('169.254.')) v4.push({ address: a.address, physical: PHYSICAL.test(name) })
      else if (a.family === 'IPv6' && !a.address.toLowerCase().startsWith('fe80')) v6.push(a.address)
    }
  }
  const ordered = [...v4.filter((a) => a.physical), ...v4.filter((a) => !a.physical)].map((a) => a.address)
  return ordered.length ? ordered.slice(0, 3) : v6.slice(0, 2)
}

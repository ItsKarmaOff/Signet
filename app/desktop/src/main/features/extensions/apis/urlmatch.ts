import { isAbsolute, relative, resolve } from 'node:path'
import { patternCovers } from './permissions'

/** A URL as the narrowest match pattern that covers it (query and fragment do not take part in matching). */
export function urlAsPattern(url: string): string | null {
  try {
    const u = new URL(url)
    if (!u.protocol || !u.hostname) return u.protocol === 'file:' ? `file://${u.pathname}` : null
    return `${u.protocol}//${u.hostname}${u.port ? `:${u.port}` : ''}${u.pathname}`
  } catch {
    return null
  }
}

/** True if the match pattern covers the URL. */
export function urlMatches(pattern: string, url: string): boolean {
  const wanted = urlAsPattern(url)
  return !!wanted && patternCovers(pattern, wanted)
}

/** What an extension holds: manifest and granted permissions, host patterns included. */
export interface Access {
  permissions: readonly string[]
  origins: readonly string[]
}

/**
 * Can the extension read or change the page at `url` (screenshot, CSS injection)? Chrome asks for a host permission or
 * `activeTab`. Signet does not track the user gesture that makes `activeTab` valid, so the permission alone counts.
 * Its own pages are always allowed.
 */
export function canAccessUrl(access: Access, extensionId: string, url: string): boolean {
  if (url.startsWith(`chrome-extension://${extensionId}/`)) return true
  if (access.permissions.includes('activeTab') || access.origins.includes('<all_urls>')) return true
  return access.origins.some((p) => urlMatches(p, url))
}

/**
 * A file of the extension named by `ref`: a path relative to its folder, a rooted path, or a `chrome-extension://` URL of
 * this extension. Null when it points elsewhere (another extension, outside the folder).
 */
export function extensionFilePath(root: string, extensionId: string, ref: string): string | null {
  let rel = ref
  if (/^chrome-extension:\/\//i.test(ref)) {
    try {
      const u = new URL(ref)
      if (u.hostname !== extensionId) return null
      rel = decodeURIComponent(u.pathname)
    } catch {
      return null
    }
  }
  const abs = resolve(root, rel.replace(/^[/\\]+/, ''))
  const inside = relative(root, abs)
  return inside && !inside.startsWith('..') && !isAbsolute(inside) ? abs : null
}

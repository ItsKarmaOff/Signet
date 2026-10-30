import type { ExtensionActionStates } from '@shared/types'
import { ApiError, type ApiCaller, type ApiHandler } from './types'

/** Chrome's default badge text color. */
export const DEFAULT_TEXT_COLOR = [255, 255, 255, 255]

interface Slot {
  enabled?: boolean
  textColor?: unknown
}

/** Is a `tabId` argument a tab? (`undefined` means the whole action.) */
function tabOf(details: unknown): number | undefined {
  const id = details !== null && typeof details === 'object' ? (details as { tabId?: unknown }).tabId : typeof details === 'number' ? details : undefined
  return typeof id === 'number' && Number.isInteger(id) && id >= 0 ? id : undefined
}

/** Colors `setBadgeTextColor` takes: `[r, g, b, a]` or a CSS color string. */
export function parseColor(color: unknown): unknown {
  if (typeof color === 'string' && color.trim()) return color.trim()
  if (Array.isArray(color) && color.length === 4 && color.every((c) => typeof c === 'number' && c >= 0 && c <= 255)) return color.map(Math.round)
  throw new ApiError('Invalid value for argument 1. Expected a color.')
}

/**
 * Enabled state and badge text color of extension buttons, globally and per tab. A tab's own value wins over the global
 * one, which wins over the default (enabled, white text). Kept in memory, like Chrome: extensions set it again at startup.
 */
export class ActionStates {
  private global = new Map<string, Slot>()
  private tabs = new Map<string, Map<number, Slot>>()
  private listeners = new Set<() => void>()

  onChange(fn: () => void): void {
    this.listeners.add(fn)
  }

  private changed(): void {
    for (const fn of this.listeners) fn()
  }

  private slot(extensionId: string, tabId: number | undefined): Slot {
    if (tabId === undefined) {
      let s = this.global.get(extensionId)
      if (!s) this.global.set(extensionId, (s = {}))
      return s
    }
    let perTab = this.tabs.get(extensionId)
    if (!perTab) this.tabs.set(extensionId, (perTab = new Map()))
    let s = perTab.get(tabId)
    if (!s) perTab.set(tabId, (s = {}))
    return s
  }

  setEnabled(extensionId: string, tabId: number | undefined, enabled: boolean): void {
    this.slot(extensionId, tabId).enabled = enabled
    this.changed()
  }

  isEnabled(extensionId: string, tabId?: number): boolean {
    return this.tabs.get(extensionId)?.get(tabId ?? -1)?.enabled ?? this.global.get(extensionId)?.enabled ?? true
  }

  setTextColor(extensionId: string, tabId: number | undefined, color: unknown): void {
    this.slot(extensionId, tabId).textColor = color
    this.changed()
  }

  textColor(extensionId: string, tabId?: number): unknown {
    return this.tabs.get(extensionId)?.get(tabId ?? -1)?.textColor ?? this.global.get(extensionId)?.textColor ?? DEFAULT_TEXT_COLOR
  }

  forgetTab(tabId: number): void {
    let any = false
    for (const perTab of this.tabs.values()) any = perTab.delete(tabId) || any
    if (any) this.changed()
  }

  forgetExtension(extensionId: string): void {
    const a = this.global.delete(extensionId)
    const b = this.tabs.delete(extensionId)
    if (a || b) this.changed()
  }

  /** Only what differs from the defaults, for the toolbar. */
  snapshot(): ExtensionActionStates {
    const out: ExtensionActionStates = {}
    for (const [id, s] of this.global) if (s.enabled !== undefined || s.textColor !== undefined) out[id] = { ...s }
    for (const [id, perTab] of this.tabs) {
      for (const [tabId, s] of perTab) {
        if (s.enabled === undefined && s.textColor === undefined) continue
        const entry = (out[id] ??= {})
        ;(entry.tabs ??= {})[tabId] = { ...s }
      }
    }
    return out
  }

  methods(): Record<string, ApiHandler> {
    return {
      'action.enable': ({ extensionId }, tab) => this.setEnabled(extensionId, tabOf(tab), true),
      'action.disable': ({ extensionId }, tab) => this.setEnabled(extensionId, tabOf(tab), false),
      'action.isEnabled': ({ extensionId }, tab) => this.isEnabled(extensionId, tabOf(tab)),
      'action.setBadgeTextColor': ({ extensionId }, details) => {
        const d = (details ?? {}) as { color?: unknown }
        this.setTextColor(extensionId, tabOf(details), parseColor(d.color))
      },
      'action.getBadgeTextColor': ({ extensionId }, details) => this.textColor(extensionId, tabOf(details))
    }
  }
}

/** Image data as it crosses from the page: `ImageData` itself does not travel, its parts do. */
export interface RawImage {
  width: number
  height: number
  data: ArrayLike<number>
}

const MAX_SIDE = 512

/** Validates an image and returns its pixels as BGRA, the order `nativeImage.createFromBitmap` expects. */
export function rgbaToBgra(image: unknown): { width: number; height: number; bitmap: Buffer } {
  const i = image as Partial<RawImage> | null
  if (!i || !Number.isInteger(i.width) || !Number.isInteger(i.height) || !i.data) throw new ApiError('Invalid image data.')
  const { width, height, data } = i as RawImage
  if (width < 1 || height < 1 || width > MAX_SIDE || height > MAX_SIDE || data.length !== width * height * 4) throw new ApiError('Invalid image data.')
  const bitmap = Buffer.alloc(data.length)
  for (let p = 0; p < data.length; p += 4) {
    bitmap[p] = data[p + 2]
    bitmap[p + 1] = data[p + 1]
    bitmap[p + 2] = data[p]
    bitmap[p + 3] = data[p + 3]
  }
  return { width, height, bitmap }
}

/**
 * `imageData` of `setIcon`: one image or a dictionary of sizes to images, as PNG data URLs keyed by width (the library's
 * icon route picks the closest size from such a dictionary).
 */
export function pngIcons(imageData: unknown, toPng: (bitmap: Buffer, width: number, height: number) => string): Record<string, string> {
  if (!imageData || typeof imageData !== 'object') throw new ApiError('Invalid image data.')
  const single = 'width' in imageData && 'data' in imageData
  const entries = single ? [[String((imageData as RawImage).width), imageData] as const] : Object.entries(imageData)
  if (!entries.length) throw new ApiError('Invalid image data.')
  const out: Record<string, string> = {}
  for (const [, image] of entries) {
    const { width, height, bitmap } = rgbaToBgra(image)
    out[String(width)] = toPng(bitmap, width, height)
  }
  return out
}

export interface SetIconDeps {
  toPng(bitmap: Buffer, width: number, height: number): string
  /** Gives the icons to the toolbar button (the library keeps its state). */
  apply(caller: ApiCaller, tabId: number | undefined, images: Record<string, string>): void
}

/** `action.setIcon` with `imageData`; icons given by path stay with the library. */
export function setIconMethod(d: SetIconDeps): ApiHandler {
  return (caller, details) => {
    const raw = (details ?? {}) as { imageData?: unknown }
    d.apply(caller, tabOf(details), pngIcons(raw.imageData, d.toPng))
  }
}

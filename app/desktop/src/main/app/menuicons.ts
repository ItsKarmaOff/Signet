import { guardMenu } from './menuguard'
import { Menu, MenuItem, nativeImage, type MenuItemConstructorOptions, type NativeImage } from 'electron'

/**
 * Icons of native menus (menu bar and context menus), as macOS SF Symbols.
 * Reserved for entries that are looked up often: the goal is to spot them at a glance, not to put them everywhere.
 * The same action keeps the same symbol in all menus. Outside macOS, no icon.
 */
const SYMBOLS = {
  settings: 'gearshape',
  newTab: 'plus',
  openInNewTab: 'plus.square.on.square',
  private: 'hand.raised',
  capture: 'camera.viewfinder',
  print: 'printer',
  find: 'magnifyingglass',
  reload: 'arrow.clockwise',
  reader: 'doc.plaintext',
  translate: 'translate',
  devTools: 'hammer',
  library: 'books.vertical',
  bookmark: 'bookmark',
  split: 'rectangle.split.2x1',
  link: 'link',
  phone: 'iphone',
  copy: 'doc.on.doc',
  download: 'square.and.arrow.down',
  pin: 'pin',
  favorite: 'star',
  pip: 'pip.enter',
  newFolder: 'folder.badge.plus',
  liveFolder: 'dot.radiowaves.up.forward',
  edit: 'pencil',
  close: 'xmark',
  delete: 'trash'
} as const

export type MenuIconName = keyof typeof SYMBOLS

/** Fixed cell (in points) in which each symbol is centered: labels stay aligned whatever the symbol width. */
const BOX_W = 18
const BOX_H = 16
/** Target height (in points) for the reference symbol (gear), close to system menu icons. */
const REF_HEIGHT = 13

const cache = new Map<MenuIconName, NativeImage | undefined>()
let unit: number | undefined

/** The symbol scaled down and centered in the cell, in raw pixels for the given scale. */
function boxed(src: NativeImage, scale: number): Buffer {
  const W = BOX_W * scale
  const H = BOX_H * scale
  const size = src.getSize()
  const k = (unit ?? 1) * scale
  const fit = Math.min(1, W / (size.width * k), H / (size.height * k))
  const img = src.resize({ width: Math.max(1, Math.round(size.width * k * fit)), height: Math.max(1, Math.round(size.height * k * fit)), quality: 'best' })
  const { width, height } = img.getSize()
  const bmp = img.toBitmap()
  const out = Buffer.alloc(W * H * 4)
  const x0 = Math.floor((W - width) / 2)
  const y0 = Math.floor((H - height) / 2)
  for (let y = 0; y < height; y++) bmp.copy(out, ((y0 + y) * W + x0) * 4, y * width * 4, (y + 1) * width * 4)
  return out
}

function build(name: MenuIconName): NativeImage | undefined {
  // createFromNamedImage returns the symbol large: we bring it down to menu size, at the gear's scale.
  unit ??= REF_HEIGHT / (nativeImage.createFromNamedImage('gearshape').getSize().height || REF_HEIGHT)
  const src = nativeImage.createFromNamedImage(SYMBOLS[name])
  if (src.isEmpty()) return undefined
  const image = nativeImage.createEmpty()
  for (const scale of [1, 2]) {
    const png = nativeImage.createFromBitmap(boxed(src, scale), { width: BOX_W * scale, height: BOX_H * scale }).toPNG()
    image.addRepresentation({ scaleFactor: scale, buffer: png })
  }
  // Template image: macOS tints it according to the theme and the entry's highlight.
  image.setTemplateImage(true)
  return image
}

/** Icon of a menu entry, or undefined (outside macOS or symbol unavailable). */
export function menuIcon(name: MenuIconName): NativeImage | undefined {
  if (process.platform !== 'darwin') return undefined
  if (!cache.has(name)) cache.set(name, build(name))
  return cache.get(name)
}

let blank: NativeImage | undefined

/** Transparent image the size of the cell: reserves the icon's space on an entry that has none. */
function blankIcon(): NativeImage {
  if (!blank) {
    blank = nativeImage.createEmpty()
    for (const scale of [1, 2]) {
      const png = nativeImage.createFromBitmap(Buffer.alloc(BOX_W * scale * BOX_H * scale * 4), { width: BOX_W * scale, height: BOX_H * scale }).toPNG()
      blank.addRepresentation({ scaleFactor: scale, buffer: png })
    }
    blank.setTemplateImage(true)
  }
  return blank
}

type Entry = MenuItemConstructorOptions | MenuItem

/**
 * Aligns the labels: in a menu (or submenu) where at least one entry has an icon, macOS shifts its text but not that
 * of the others. So we give an empty icon to the other entries of that level. Levels without icons are left as they are.
 */
export function alignIcons<T extends Entry>(items: T[]): T[] {
  const any = items.some((item) => item.icon && item.type !== 'separator')
  return items.map((item) => {
    if (item instanceof MenuItem) {
      // Entry already built (extensions): we complete the object itself.
      if (any && !item.icon && item.type !== 'separator') item.icon = blankIcon()
      return item
    }
    return {
      ...item,
      ...(Array.isArray(item.submenu) && { submenu: alignIcons(item.submenu) }),
      ...(any && !item.icon && item.type !== 'separator' && { icon: blankIcon() })
    }
  })
}

/** Menu.buildFromTemplate, with aligned labels (see alignIcons) and Escape closing it (see menuguard). */
export function buildMenu(items: Entry[]): Menu {
  return guardMenu(Menu.buildFromTemplate(alignIcons(items)))
}

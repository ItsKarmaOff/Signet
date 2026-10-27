import { app } from 'electron'
import type { Menu } from 'electron'

/** What the guard needs from a menu (a `Menu` fits). */
export interface PopupMenu {
  popup: (options?: never) => void
  closePopup: () => void
  once: (event: 'menu-will-close', cb: () => void) => unknown
}

/**
 * Safety net for native context menus. The OS normally gives the open menu the keyboard and closes it on Escape.
 * When it does not (Linux when the menu is shown from an asynchronous callback, no grab), the key reaches the
 * focused view instead: the guard sees it first and closes the menu itself.
 */
export class MenuGuard {
  private open: PopupMenu | null = null

  /** A menu is being shown. */
  opened(menu: PopupMenu): void {
    this.open = menu
  }

  /** The menu is gone (any way). */
  closed(menu: PopupMenu): void {
    if (this.open === menu) this.open = null
  }

  /** Key press seen by a view: true if it was an Escape that closed the open menu (to swallow). */
  escape(input: { type: string; key: string }): boolean {
    if (!this.open || input.type !== 'keyDown' || input.key !== 'Escape') return false
    const menu = this.open
    this.open = null
    menu.closePopup()
    return true
  }
}

export const menuGuard = new MenuGuard()

const tracked = new WeakSet<Menu>()

/** Makes `menu.popup()` register with the guard; returns the same menu. */
export function guardMenu(menu: Menu): Menu {
  if (tracked.has(menu)) return menu
  tracked.add(menu)
  const popup = menu.popup.bind(menu) as Menu['popup']
  menu.popup = (options) => {
    menuGuard.opened(menu)
    menu.once('menu-will-close', () => menuGuard.closed(menu))
    return popup(options)
  }
  return menu
}

/** Escape seen by any view of the app closes the open native menu. */
export function initMenuGuard(): void {
  app.on('web-contents-created', (_e, wc) => {
    wc.on('before-input-event', (event, input) => {
      if (menuGuard.escape(input)) event.preventDefault()
    })
  })
}

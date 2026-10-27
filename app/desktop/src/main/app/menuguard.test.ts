import { describe, expect, it, vi } from 'vitest'

vi.mock('electron', () => ({ app: { on: vi.fn() } }))

const { MenuGuard } = await import('./menuguard')

const menu = () => ({ popup: vi.fn(), closePopup: vi.fn(), once: vi.fn() })
const esc = { type: 'keyDown', key: 'Escape' }

describe('MenuGuard', () => {
  it('closes the open menu on Escape and swallows the key', () => {
    const g = new MenuGuard()
    const m = menu()
    g.opened(m)
    expect(g.escape(esc)).toBe(true)
    expect(m.closePopup).toHaveBeenCalledTimes(1)
    expect(g.escape(esc)).toBe(false)
  })

  it('lets Escape through when no menu is open', () => {
    const g = new MenuGuard()
    expect(g.escape(esc)).toBe(false)
  })

  it('ignores other keys and key releases', () => {
    const g = new MenuGuard()
    const m = menu()
    g.opened(m)
    expect(g.escape({ type: 'keyDown', key: 'a' })).toBe(false)
    expect(g.escape({ type: 'keyUp', key: 'Escape' })).toBe(false)
    expect(m.closePopup).not.toHaveBeenCalled()
  })

  it('forgets a menu that closed by itself', () => {
    const g = new MenuGuard()
    const m = menu()
    g.opened(m)
    g.closed(m)
    expect(g.escape(esc)).toBe(false)
  })

  it('a stale close does not drop a newer menu', () => {
    const g = new MenuGuard()
    const a = menu()
    const b = menu()
    g.opened(a)
    g.opened(b)
    g.closed(a)
    expect(g.escape(esc)).toBe(true)
    expect(b.closePopup).toHaveBeenCalled()
  })
})

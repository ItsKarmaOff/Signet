/// <reference lib="dom" />
// Page script: passwords from Signet's password vault (all frames, preload's isolated world).
// Offers the site's accounts when a login field is touched (never silent filling),
// and tells the main process which credentials were submitted so it can offer to save them.
// The origin is determined by the main process (sending frame): the page cannot make it believe anything.
import { ipcRenderer } from 'electron'

interface Account {
  id: string
  username: string
}

if (/^https?:$/.test(location.protocol)) start()

function start(): void {
  const fr = /^fr\b/i.test(navigator.language)
  const TEXT = {
    title: fr ? 'Mots de passe Signet' : 'Signet passwords',
    noUsername: fr ? 'Sans identifiant' : 'No username'
  }
  const USERNAME_HINT = /user|login|identifier|account|e-?mail|courriel|identifiant/i

  // ───────────── Field detection ─────────────

  const isInput = (el: EventTarget | null): el is HTMLInputElement => el instanceof HTMLInputElement
  const visible = (el: HTMLElement) => el.getClientRects().length > 0 && getComputedStyle(el).visibility !== 'hidden'
  const isPassword = (el: HTMLInputElement) => el.type === 'password'
  const isTextLike = (el: HTMLInputElement) => ['text', 'email', 'tel', ''].includes(el.type) && !el.readOnly && !el.disabled
  const auto = (el: HTMLInputElement) => (el.getAttribute('autocomplete') ?? '').toLowerCase()

  /** The field's form, or failing that the closest ancestor that contains a password field (pages without <form>). */
  function containerOf(el: Element): ParentNode {
    if (isInput(el) && el.form) return el.form
    let node: Element | null = el.parentElement
    for (let i = 0; node && i < 8; i++, node = node.parentElement) if (node.querySelector('input[type="password"]')) return node
    return document
  }

  const inputsOf = (root: ParentNode) => [...root.querySelectorAll('input')].filter((i) => visible(i))
  // Direct selector: no visibility test (forced style/layout) on fields that are not passwords.
  const passwordsOf = (root: ParentNode) => [...root.querySelectorAll<HTMLInputElement>('input[type="password"]')].filter(visible)

  /** Username field of a set: `autocomplete=username|email`, otherwise the last text field before the password. */
  function usernameOf(root: ParentNode, password: HTMLInputElement | null): HTMLInputElement | null {
    const inputs = inputsOf(root)
    const tagged = inputs.find((i) => isTextLike(i) && /\b(username|email)\b/.test(auto(i)))
    if (tagged) return tagged
    const before = password ? inputs.slice(0, inputs.indexOf(password)) : inputs
    return [...before].reverse().find(isTextLike) ?? null
  }

  /** True for a field that deserves the account list. */
  function isLoginField(el: HTMLInputElement): boolean {
    if (isPassword(el)) return auto(el) !== 'new-password'
    if (!isTextLike(el)) return false
    if (/\b(username|webauthn)\b/.test(auto(el))) return true
    const root = containerOf(el)
    const passwords = passwordsOf(root)
    if (passwords.length) return usernameOf(root, passwords[0]) === el
    // Two-step login: username-only page.
    return USERNAME_HINT.test(`${el.name} ${el.id}`) && el.type !== 'tel'
  }

  // ───────────── Account list ─────────────

  let host: HTMLElement | null = null
  let shadow: ShadowRoot | null = null
  let field: HTMLInputElement | null = null
  let accounts: Account[] = []
  let active = -1
  let items: HTMLElement[] = []
  /** Right after a fill, focus moves to the password: no new list. */
  let quietUntil = 0
  let cache: { at: number; list: Account[] } | null = null

  async function fetchAccounts(): Promise<Account[]> {
    if (cache && Date.now() - cache.at < 10_000) return cache.list
    try {
      const list = (await ipcRenderer.invoke('pw:accounts')) as Account[]
      cache = { at: Date.now(), list: Array.isArray(list) ? list : [] }
    } catch {
      cache = { at: Date.now(), list: [] }
    }
    return cache.list
  }

  const CSS = `
    :host { all: initial }
    .menu { position: fixed; z-index: 2147483647; min-width: 220px; max-width: 360px; box-sizing: border-box; padding: 4px;
      background: rgba(252,252,253,.97); color: #1d1d1f; border: .5px solid rgba(0,0,0,.14); border-radius: 10px;
      box-shadow: 0 10px 30px rgba(0,0,0,.18), 0 1px 3px rgba(0,0,0,.1); font: 13px/1.3 -apple-system, BlinkMacSystemFont, system-ui, sans-serif;
      animation: in .12s ease-out; transform-origin: top left }
    @keyframes in { from { opacity: 0; transform: scale(.97) } }
    @media (prefers-reduced-motion: reduce) { .menu { animation: none } }
    @media (prefers-color-scheme: dark) { .menu { background: rgba(40,40,44,.97); color: #ececf0; border-color: rgba(255,255,255,.14) } }
    .title { padding: 5px 8px 4px; font-size: 11px; opacity: .6 }
    .item { display: flex; align-items: center; gap: 8px; padding: 6px 8px; border-radius: 6px; cursor: default; user-select: none }
    .item.on { background: hsl(250 70% 55%); color: #fff }
    .key { flex: none; width: 16px; height: 16px; opacity: .7 }
    .name { overflow: hidden; text-overflow: ellipsis; white-space: nowrap }
    .muted { opacity: .65; font-style: italic }
  `
  const KEY_ICON =
    '<svg class="key" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><circle cx="7.5" cy="15.5" r="5.5"/><path d="m21 2-9.6 9.6M15.5 7.5l3 3L22 7l-3-3"/></svg>'

  /** Highlights an account without rebuilding the list (a click in progress must keep its target). */
  function highlight(i: number): void {
    active = i
    items.forEach((el, j) => el.classList.toggle('on', j === i))
  }

  function hide(): void {
    host?.remove()
    host = null
    shadow = null
    field = null
    active = -1
    items = []
  }

  function render(): void {
    if (!shadow || !field) return
    const r = field.getBoundingClientRect()
    const menu = document.createElement('div')
    menu.className = 'menu'
    menu.style.left = `${Math.max(4, Math.min(r.left, innerWidth - 224))}px`
    menu.style.top = `${r.bottom + 4 + 200 > innerHeight && r.top > 200 ? Math.max(4, r.top - 4 - 36 * (accounts.length + 1)) : r.bottom + 4}px`
    menu.style.width = `${Math.max(220, Math.min(360, r.width))}px`
    const title = document.createElement('div')
    title.className = 'title'
    title.textContent = TEXT.title
    menu.append(title)
    items = accounts.map((a, i) => {
      const item = document.createElement('div')
      item.className = `item${i === active ? ' on' : ''}`
      item.innerHTML = KEY_ICON
      const name = document.createElement('span')
      name.className = `name${a.username ? '' : ' muted'}`
      name.textContent = a.username || TEXT.noUsername
      item.append(name)
      // The field keeps focus; only a real click fills.
      item.addEventListener('mousedown', (e) => e.preventDefault())
      item.addEventListener('mouseenter', () => highlight(i))
      item.addEventListener('click', (e) => {
        if (e.isTrusted && Date.now() - shownAt > 300) void pick(i)
      })
      menu.append(item)
      return item
    })
    shadow.replaceChildren(Object.assign(document.createElement('style'), { textContent: CSS }), menu)
  }

  let shownAt = 0

  async function show(el: HTMLInputElement): Promise<void> {
    const list = await fetchAccounts()
    if (!list.length || document.activeElement !== el) return
    // Username field already filled: only offer accounts that start the same way.
    const typed = !isPassword(el) ? el.value.trim().toLowerCase() : ''
    accounts = typed ? list.filter((a) => a.username.toLowerCase().startsWith(typed)) : list
    if (!accounts.length) return hide()
    if (!host) {
      host = document.createElement('signet-passwords')
      shadow = host.attachShadow({ mode: 'closed' })
      document.documentElement.append(host)
    }
    // The page must not be able to hide the menu (clickjacking): inline styles take priority.
    for (const [k, v] of Object.entries({
      all: 'initial',
      display: 'block',
      opacity: '1',
      visibility: 'visible',
      filter: 'none',
      'clip-path': 'none',
      transform: 'none',
      'mix-blend-mode': 'normal',
      'pointer-events': 'auto',
      position: 'static'
    }))
      host.style.setProperty(k, v, 'important')
    shownAt = Date.now()
    field = el
    active = -1
    render()
  }

  const setValue = (el: HTMLInputElement, value: string) => {
    Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value')?.set?.call(el, value)
    el.dispatchEvent(new Event('input', { bubbles: true }))
    el.dispatchEvent(new Event('change', { bubbles: true }))
  }

  async function pick(i: number): Promise<void> {
    const target = field
    const account = accounts[i]
    hide()
    if (!target || !account) return
    type Credential = { username: string; password: string } | null
    let cred: Credential
    try {
      cred = (await ipcRenderer.invoke('pw:fill', account.id)) as Credential
    } catch {
      return
    }
    if (!cred) return
    const root = containerOf(target)
    const password = isPassword(target) ? target : (passwordsOf(root).find((p) => auto(p) !== 'new-password') ?? null)
    const username = isPassword(target) ? usernameOf(root, target) : target
    if (username && cred.username) setValue(username, cred.username)
    if (password) {
      setValue(password, cred.password)
      quietUntil = Date.now() + 600
      password.focus()
    }
  }

  document.addEventListener(
    'focusin',
    (e) => {
      const el = e.target
      if (!isInput(el) || !isLoginField(el)) return hide()
      if (Date.now() > quietUntil) void show(el)
    },
    true
  )
  document.addEventListener(
    'mousedown',
    (e) => {
      const el = e.target
      if (e.isTrusted && isInput(el) && el === document.activeElement && !host && isLoginField(el)) void show(el)
    },
    true
  )
  document.addEventListener(
    'focusout',
    (e) => {
      if (e.target === field) hide()
    },
    true
  )
  document.addEventListener(
    'input',
    (e) => {
      if (e.isTrusted && field && e.target === field) void show(field)
    },
    true
  )
  addEventListener('resize', hide)
  addEventListener('scroll', () => host && render(), true)

  // ───────────── Sending credentials ─────────────

  let lastSent = ''
  let lastAt = 0

  /** Typed password: the new one (sign-up, change), otherwise the first non-empty one. */
  function chosenPassword(root: ParentNode): HTMLInputElement | null {
    const filled = passwordsOf(root).filter((p) => p.value)
    if (!filled.length) return null
    const fresh = filled.filter((p) => auto(p) === 'new-password')
    if (fresh.length) return fresh[0]
    const twice = filled.find((p, i) => filled.some((q, j) => j !== i && q.value === p.value))
    if (twice) return twice
    return filled.length === 3 ? filled[1] : filled[0]
  }

  function report(from: Element): void {
    const root = containerOf(from)
    const password = chosenPassword(root)
    let username: string
    if (password) username = usernameOf(root, password)?.value.trim() ?? ''
    else {
      // "Username" step alone: kept by the main process for the password page.
      const id = isInput(from) && isTextLike(from) ? from : usernameOf(root, null)
      if (!id || !id.value.trim() || !(/\b(username|email)\b/.test(auto(id)) || USERNAME_HINT.test(`${id.name} ${id.id} ${id.type}`))) return
      username = id.value.trim()
    }
    const pw = password?.value ?? ''
    const key = `${username}\n${pw}`
    if (key === lastSent && Date.now() - lastAt < 2000) return
    lastSent = key
    lastAt = Date.now()
    ipcRenderer.send('pw:submit', username, pw)
  }

  document.addEventListener(
    'submit',
    (e) => {
      // Submissions simulated by the page (requestSubmit in a loop) ignored.
      if (!e.isTrusted && !navigator.userActivation?.isActive) return
      if (e.target instanceof HTMLFormElement) report(e.target)
    },
    true
  )
  document.addEventListener(
    'keydown',
    (e) => {
      // Navigating the account list: real keys only (a page that gives the field focus itself
      // must not be able to pick an account with simulated keys, then read the filled password).
      if (host && e.target === field && e.isTrusted) {
        if (e.key === 'ArrowDown' || e.key === 'ArrowUp') {
          e.preventDefault()
          return highlight((active + (e.key === 'ArrowDown' ? 1 : accounts.length + (active < 0 ? 0 : -1))) % accounts.length)
        }
        if (e.key === 'Enter' && active >= 0) {
          e.preventDefault()
          e.stopPropagation()
          return void pick(active)
        }
        if (e.key === 'Escape') return hide()
      }
      if (e.key === 'Enter' && e.isTrusted && isInput(e.target) && (isPassword(e.target) || isTextLike(e.target))) report(e.target)
    },
    true
  )
  document.addEventListener(
    'click',
    (e) => {
      if (!e.isTrusted || !(e.target instanceof Element)) return
      const button = e.target.closest('button, input[type="submit"], input[type="image"], [role="button"]')
      if (!button) return
      if (button instanceof HTMLButtonElement && button.type !== 'submit' && button.form) return // simple button of a real form
      const root = containerOf(button)
      if (root !== document) return report(button)
      // Page without a form: we only report if a password is filled (value tested before visibility).
      for (const p of document.querySelectorAll<HTMLInputElement>('input[type="password"]')) if (p.value && visible(p)) return report(button)
    },
    true
  )
}

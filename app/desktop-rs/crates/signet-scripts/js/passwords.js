// Port of preload/passwords.ts (SPEC v17). Passwords from Signet's vault. Main frame only, document start, http(s) pages.
// Offers the site's accounts when a login field is touched (never silent filling), and tells the browser which
// credentials were submitted so it can offer to save them. The origin is determined by the browser side (sending
// frame): the page cannot make it believe anything.
// Channels: pw:accounts (request), pw:fill (request, credentials come back in the reply), pw:submit (send).
(function passwords(bridge, config, g) {
  'use strict'
  g = g || globalThis
  if (g.top !== g || !/^https?:$/.test(g.location.protocol)) return

  const doc = g.document
  const InputCtor = g.HTMLInputElement
  const fr = /^fr\b/i.test(g.navigator.language)
  const TEXT = {
    title: fr ? 'Mots de passe Signet' : 'Signet passwords',
    noUsername: fr ? 'Sans identifiant' : 'No username'
  }
  const USERNAME_HINT = /user|login|identifier|account|e-?mail|courriel|identifiant/i
  const nowMs = () => g.Date.now()

  // ───────────── Field detection ─────────────

  const isInput = (el) => el instanceof InputCtor
  const visible = (el) => el.getClientRects().length > 0 && g.getComputedStyle(el).visibility !== 'hidden'
  const isPassword = (el) => el.type === 'password'
  const isTextLike = (el) => ['text', 'email', 'tel', ''].includes(el.type) && !el.readOnly && !el.disabled
  const auto = (el) => (el.getAttribute('autocomplete') || '').toLowerCase()

  /** The field's form, or failing that the closest ancestor that contains a password field (pages without <form>). */
  function containerOf(el) {
    if (isInput(el) && el.form) return el.form
    let node = el.parentElement
    for (let i = 0; node && i < 8; i++, node = node.parentElement) if (node.querySelector('input[type="password"]')) return node
    return doc
  }

  const inputsOf = (root) => [...root.querySelectorAll('input')].filter((i) => visible(i))
  // Direct selector: no visibility test (forced style/layout) on fields that are not passwords.
  const passwordsOf = (root) => [...root.querySelectorAll('input[type="password"]')].filter(visible)

  /** Username field of a set: `autocomplete=username|email`, otherwise the last text field before the password. */
  function usernameOf(root, password) {
    const inputs = inputsOf(root)
    const tagged = inputs.find((i) => isTextLike(i) && /\b(username|email)\b/.test(auto(i)))
    if (tagged) return tagged
    const before = password ? inputs.slice(0, inputs.indexOf(password)) : inputs
    return [...before].reverse().find(isTextLike) || null
  }

  /** True for a field that deserves the account list. */
  function isLoginField(el) {
    if (isPassword(el)) return auto(el) !== 'new-password'
    if (!isTextLike(el)) return false
    if (/\b(username|webauthn)\b/.test(auto(el))) return true
    const root = containerOf(el)
    const pws = passwordsOf(root)
    if (pws.length) return usernameOf(root, pws[0]) === el
    // Two-step login: username-only page.
    return USERNAME_HINT.test(el.name + ' ' + el.id) && el.type !== 'tel'
  }

  // ───────────── Account list ─────────────

  let host = null
  let shadow = null
  let field = null
  let accounts = []
  let active = -1
  let items = []
  /** Right after a fill, focus moves to the password: no new list. */
  let quietUntil = 0
  let cache = null
  let shownAt = 0
  let showToken = 0

  /** The site's accounts `[{ id, username }]`, cached for 10 s. `cb(list)` is always called. */
  function fetchAccounts(cb) {
    if (cache && nowMs() - cache.at < 10000) return cb(cache.list)
    bridge.request(
      'pw:accounts',
      null,
      (error, list) => {
        const clean = []
        if (error === null && Array.isArray(list)) {
          for (const a of list) if (a && typeof a.id === 'string' && typeof a.username === 'string') clean.push({ id: a.id, username: a.username })
        }
        cache = { at: nowMs(), list: clean }
        cb(clean)
      },
      5000
    )
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
  const SVG_NS = 'http://www.w3.org/2000/svg'
  /** Key icon built with DOM calls (no innerHTML: Trusted Types pages refuse it). */
  function keyIcon() {
    const svg = doc.createElementNS(SVG_NS, 'svg')
    svg.setAttribute('class', 'key')
    const attrs = { viewBox: '0 0 24 24', fill: 'none', stroke: 'currentColor', 'stroke-width': '2', 'stroke-linecap': 'round', 'stroke-linejoin': 'round' }
    for (const k of Object.keys(attrs)) svg.setAttribute(k, attrs[k])
    const circle = doc.createElementNS(SVG_NS, 'circle')
    circle.setAttribute('cx', '7.5')
    circle.setAttribute('cy', '15.5')
    circle.setAttribute('r', '5.5')
    const path = doc.createElementNS(SVG_NS, 'path')
    path.setAttribute('d', 'm21 2-9.6 9.6M15.5 7.5l3 3L22 7l-3-3')
    svg.append(circle, path)
    return svg
  }

  /** Highlights an account without rebuilding the list (a click in progress must keep its target). */
  function highlight(i) {
    active = i
    items.forEach((el, j) => el.classList.toggle('on', j === i))
  }

  function hide() {
    showToken++
    if (host) host.remove()
    host = null
    shadow = null
    field = null
    active = -1
    items = []
  }

  function render() {
    if (!shadow || !field) return
    const r = field.getBoundingClientRect()
    const menu = doc.createElement('div')
    menu.className = 'menu'
    menu.style.left = Math.max(4, Math.min(r.left, g.innerWidth - 224)) + 'px'
    menu.style.top =
      (r.bottom + 4 + 200 > g.innerHeight && r.top > 200 ? Math.max(4, r.top - 4 - 36 * (accounts.length + 1)) : r.bottom + 4) + 'px'
    menu.style.width = Math.max(220, Math.min(360, r.width)) + 'px'
    const title = doc.createElement('div')
    title.className = 'title'
    title.textContent = TEXT.title
    menu.append(title)
    items = accounts.map((a, i) => {
      const item = doc.createElement('div')
      item.className = 'item' + (i === active ? ' on' : '')
      item.append(keyIcon())
      const name = doc.createElement('span')
      name.className = 'name' + (a.username ? '' : ' muted')
      name.textContent = a.username || TEXT.noUsername
      item.append(name)
      // The field keeps focus; only a real click fills.
      item.addEventListener('mousedown', (e) => e.preventDefault())
      item.addEventListener('mouseenter', () => highlight(i))
      item.addEventListener('click', (e) => {
        if (e.isTrusted && nowMs() - shownAt > 300) pick(i)
      })
      menu.append(item)
      return item
    })
    const style = doc.createElement('style')
    style.textContent = CSS
    shadow.replaceChildren(style, menu)
  }

  const HOST_STYLE = {
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
  }

  function show(el) {
    const token = ++showToken
    fetchAccounts((list) => {
      if (token !== showToken || !list.length || doc.activeElement !== el) return
      // Username field already filled: only offer accounts that start the same way.
      const typed = !isPassword(el) ? el.value.trim().toLowerCase() : ''
      accounts = typed ? list.filter((a) => a.username.toLowerCase().startsWith(typed)) : list
      if (!accounts.length) return hide()
      if (!host) {
        host = doc.createElement('signet-passwords')
        shadow = host.attachShadow({ mode: 'closed' })
        doc.documentElement.append(host)
      }
      // The page must not be able to hide the menu (clickjacking): inline styles take priority.
      for (const k of Object.keys(HOST_STYLE)) host.style.setProperty(k, HOST_STYLE[k], 'important')
      shownAt = nowMs()
      field = el
      active = -1
      render()
    })
  }

  const valueSetter = Object.getOwnPropertyDescriptor(InputCtor.prototype, 'value').set
  const setValue = (el, value) => {
    valueSetter.call(el, value)
    el.dispatchEvent(new g.Event('input', { bubbles: true }))
    el.dispatchEvent(new g.Event('change', { bubbles: true }))
  }

  function pick(i) {
    const target = field
    const account = accounts[i]
    hide()
    if (!target || !account) return
    bridge.request(
      'pw:fill',
      { id: account.id },
      (error, cred) => {
        if (error !== null || !cred || typeof cred.password !== 'string') return
        const root = containerOf(target)
        const password = isPassword(target) ? target : passwordsOf(root).find((p) => auto(p) !== 'new-password') || null
        const username = isPassword(target) ? usernameOf(root, target) : target
        if (username && typeof cred.username === 'string' && cred.username) setValue(username, cred.username)
        if (password) {
          setValue(password, cred.password)
          quietUntil = nowMs() + 600
          password.focus()
        }
      },
      30000
    )
  }

  doc.addEventListener(
    'focusin',
    (e) => {
      const el = e.target
      if (!isInput(el) || !isLoginField(el)) return hide()
      if (nowMs() > quietUntil) show(el)
    },
    true
  )
  doc.addEventListener(
    'mousedown',
    (e) => {
      const el = e.target
      if (e.isTrusted && isInput(el) && el === doc.activeElement && !host && isLoginField(el)) show(el)
    },
    true
  )
  doc.addEventListener(
    'focusout',
    (e) => {
      if (e.target === field) hide()
    },
    true
  )
  doc.addEventListener(
    'input',
    (e) => {
      if (e.isTrusted && field && e.target === field) show(field)
    },
    true
  )
  g.addEventListener('resize', hide)
  g.addEventListener('scroll', () => host && render(), true)

  // ───────────── Sending credentials ─────────────

  let lastSent = ''
  let lastAt = 0

  /** Typed password: the new one (sign-up, change), otherwise the first non-empty one. */
  function chosenPassword(root) {
    const filled = passwordsOf(root).filter((p) => p.value)
    if (!filled.length) return null
    const fresh = filled.filter((p) => auto(p) === 'new-password')
    if (fresh.length) return fresh[0]
    const twice = filled.find((p, i) => filled.some((q, j) => j !== i && q.value === p.value))
    if (twice) return twice
    return filled.length === 3 ? filled[1] : filled[0]
  }

  function report(from) {
    const root = containerOf(from)
    const password = chosenPassword(root)
    let username
    if (password) {
      const u = usernameOf(root, password)
      username = u ? u.value.trim() : ''
    } else {
      // "Username" step alone: kept by the browser side for the password page.
      const id = isInput(from) && isTextLike(from) ? from : usernameOf(root, null)
      if (!id || !id.value.trim() || !(/\b(username|email)\b/.test(auto(id)) || USERNAME_HINT.test(id.name + ' ' + id.id + ' ' + id.type))) return
      username = id.value.trim()
    }
    const pw = password ? password.value : ''
    const key = username + '\n' + pw
    if (key === lastSent && nowMs() - lastAt < 2000) return
    lastSent = key
    lastAt = nowMs()
    bridge.send('pw:submit', { username, password: pw })
  }

  doc.addEventListener(
    'submit',
    (e) => {
      // Submissions simulated by the page (requestSubmit in a loop) ignored.
      if (!e.isTrusted && !(g.navigator.userActivation && g.navigator.userActivation.isActive)) return
      if (e.target instanceof g.HTMLFormElement) report(e.target)
    },
    true
  )
  doc.addEventListener(
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
          return pick(active)
        }
        if (e.key === 'Escape') return hide()
      }
      if (e.key === 'Enter' && e.isTrusted && isInput(e.target) && (isPassword(e.target) || isTextLike(e.target))) report(e.target)
    },
    true
  )
  doc.addEventListener(
    'click',
    (e) => {
      if (!e.isTrusted || !(e.target instanceof g.Element)) return
      const button = e.target.closest('button, input[type="submit"], input[type="image"], [role="button"]')
      if (!button) return
      if (button instanceof g.HTMLButtonElement && button.type !== 'submit' && button.form) return // simple button of a real form
      const root = containerOf(button)
      if (root !== doc) return report(button)
      // Page without a form: we only report if a password is filled (value tested before visibility).
      for (const p of doc.querySelectorAll('input[type="password"]')) if (p.value && visible(p)) return report(button)
    },
    true
  )
})

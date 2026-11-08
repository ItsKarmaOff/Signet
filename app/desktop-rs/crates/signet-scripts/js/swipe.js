// Port of preload/swipe.ts (SPEC v10). Two-finger swipe to go back / forward. Main frame only, document start.
// Runs in the page's world now (no isolated world): the bubble lives in a closed shadow root, nothing is exposed.
// Channels: page:submit (send), swipe:can (request), swipe:navigate (send).
(function swipe(bridge, config, g) {
  'use strict'
  g = g || globalThis
  if (g.top !== g) return

  const THRESHOLD = 120 // cumulative movement (px) from which releasing navigates
  const BUBBLE = 40
  const SHOWN = 44 // visible px at the threshold
  const OVER = 12 // px more at most beyond the threshold
  const END_GAP = 150 // silence (ms) that ends a gesture, momentum included
  const OVERSHOOT = 2.5 // beyond this multiple of the threshold, we navigate without waiting for release
  const FALL_PRE = 4 // decreasing events (before the threshold) that signal a released gesture
  const FALL_POST = 3 // same after the threshold: trackpad momentum has started, the fingers are lifted
  const FALL_RATIO = 0.9 // the last |deltaX| must be at most this ratio of the first of the series
  const RISE = 3 // increasing events that signal a new gesture during the previous one's momentum
  const DIAG = !!(config && config.swipeDiag) // development diagnostics
  const LOCK_AT = 6 // cumulative movement (px) from which we decide: horizontal or not

  const doc = g.document
  const now = () => g.performance.now()
  const diag = (s) => {
    if (DIAG) g.console.log('SIGNETSWIPE ' + s)
  }

  pageHelpers()
  start()

  function pageHelpers() {
    // A form submitted by the user: the browser does not open the resulting navigation in Peek (POST, SSO, payment).
    // Capture, to run before the page's handlers; real submissions only (`isTrusted`).
    g.addEventListener(
      'submit',
      (e) => {
        if (e.isTrusted) bridge.send('page:submit', null)
      },
      true
    )

    // Extension store pages: hides the banner and the window offering to switch browsers, although installation works
    // here. Its domain only (spotted by semantic attributes).
    if (g.location.origin === 'https://chromewebstore.google.com') {
      let queued = false
      const hide = () => {
        queued = false
        const promo = doc.getElementById('promo-header')
        const aside = promo && promo.closest('aside')
        if (aside) aside.style.display = 'none'
        // The window containing the insert would stay displayed as an empty white circle.
        const dialog = doc.querySelector('[role="dialog"][aria-labelledby="promo-header"]')
        if (dialog) dialog.style.display = 'none'
        const button = doc.querySelector('button[jsname="oGfC5c"]')
        let banner = button && button.parentElement
        while (banner && !banner.querySelector('[role="img"]') && banner.parentElement) banner = banner.parentElement
        if (banner && banner.querySelectorAll('button').length < 3) banner.closest('#c3, section, div').style.display = 'none'
      }
      // At most one pass per frame; the observer stays active (the page changes without reloading).
      new g.MutationObserver(() => {
        if (queued) return
        queued = true
        g.requestAnimationFrame(hide)
      }).observe(doc, { childList: true, subtree: true })
      hide()
    }
  }

  function start() {
    /** idle: no gesture; pending: axis not yet decided; track: tracking; ignore: gesture to let through. */
    let state = 'idle'
    let accX = 0
    let accY = 0
    let dir = 'back'
    let progress = 0
    let can = { enabled: true, back: false, forward: false }
    let hist = [] // |deltaX| of the gesture's latest events
    let dxs = [] // signed deltaX of the latest events
    let spent = false // gesture settled (navigation or bubble return): waiting for its momentum to end
    let disabled = false // the browser refused: nothing more to do in this document
    let endTimer
    let lastWheel = 0 // timestamp of the last wheel event
    let raf = 0
    let ui = null
    let pendingInfo = null

    const reducedMotion = () => g.matchMedia('(prefers-reduced-motion: reduce)').matches

    /** Can an ancestor (or the page) still scroll horizontally in this direction (dx > 0: to the right)? */
    function canScroll(path, dx) {
      const hit = scrollBlocker(path, dx)
      if (hit && DIAG) diag('refused: still scrolls <' + hit.localName + (hit.id ? '#' + hit.id : '') + '> sw=' + hit.scrollWidth + ' cw=' + hit.clientWidth + ' sl=' + hit.scrollLeft)
      return hit !== null
    }

    function scrollBlocker(path, dx) {
      for (const node of path) {
        if (!(node instanceof g.Element)) continue
        const root = node === doc.documentElement || node === doc.body
        const el = root ? (doc.scrollingElement || doc.documentElement) : node
        if (el.scrollWidth <= el.clientWidth + 1) continue
        const ox = g.getComputedStyle(node).overflowX
        if (root ? ox === 'hidden' || ox === 'clip' : ox !== 'auto' && ox !== 'scroll' && ox !== 'overlay') continue
        const room = dx > 0 ? el.scrollLeft + el.clientWidth < el.scrollWidth - 1 : el.scrollLeft > 0
        if (room) return el
        if (root) break
      }
      return null
    }

    /** The last n |deltaX| decrease without rising and end under FALL_RATIO of the first: momentum of a released gesture. */
    function falling(n) {
      if (hist.length < n) return false
      const h = hist.slice(-n)
      for (let i = 1; i < n; i++) if (h[i] > h[i - 1]) return false
      return h[0] > 0 && h[n - 1] <= FALL_RATIO * h[0]
    }

    /** Recent movement (sum of the last 3 deltaX) in the gesture's direction: negative = the fingers are coming back. */
    function recentSigned() {
      const sum = dxs.slice(-3).reduce((a, b) => a + b, 0)
      return dir === 'back' ? -sum : sum
    }

    /** Distance the trackpad momentum will still cover: last |deltaX| * r / (1 - r), r = observed decay. */
    function momentum() {
      const n = hist.length
      if (n < 2 || recentSigned() <= 0) return 0
      const last = hist[n - 1]
      const r = hist[n - 2] > 0 ? Math.min(0.92, last / hist[n - 2]) : 0
      return (last * r) / (1 - r)
    }

    function rising() {
      if (hist.length < RISE) return false
      const h = hist.slice(-RISE)
      return h.every((v, i) => i === 0 || v > h[i - 1])
    }

    /** Settles the gesture without waiting for the end of the momentum: the rest of the stream is ignored until silence. */
    function settle(ok) {
      const bubble = ui
      ui = null
      state = 'ignore'
      spent = true
      hist = []
      dxs = []
      diag('settled early: ' + (ok ? 'navigates' : 'nothing') + ' (' + dir + ', progress=' + Math.round(progress) + ')')
      if (ok) bridge.send('swipe:navigate', dir)
      if (bubble) bubble.release(ok, dir)
    }

    /** Timer deadline: complete silence, otherwise we re-arm for the remaining time. */
    function onQuiet() {
      const left = END_GAP - (now() - lastWheel)
      if (left > 1) endTimer = g.setTimeout(onQuiet, left)
      else finish()
    }

    function finish() {
      endTimer = undefined
      spent = false
      hist = []
      dxs = []
      const was = state
      diag('gesture end (state ' + was + ', bubble=' + !!ui + ', progress=' + Math.round(progress) + ')')
      state = 'idle'
      accX = accY = progress = 0
      if (was !== 'track' || !ui) return
      const bubble = ui
      ui = null
      const ok = bubble.reached && can[dir] && recentSigned() >= 0
      diag('end: reached=' + bubble.reached + ' ' + dir + '=' + can[dir] + ' -> ' + (ok ? 'navigates' : 'nothing'))
      if (ok) bridge.send('swipe:navigate', dir)
      bubble.release(ok, dir)
    }

    function render() {
      raf = 0
      if (ui) ui.update(progress, dir, can[dir])
    }

    function begin(e) {
      const horizontal = Math.abs(accX) > 1.5 * Math.abs(accY)
      if (!horizontal || e.defaultPrevented) {
        diag('refused: ' + (!horizontal ? 'not horizontal' : 'event canceled by the page') + ' accX=' + accX + ' accY=' + accY)
        state = 'ignore'
        return
      }
      // The gesture's direction: content pulled to the right (negative deltaX) = previous.
      dir = accX < 0 ? 'back' : 'forward'
      if (canScroll(e.composedPath(), accX)) {
        state = 'ignore'
        return
      }
      state = 'track'
      diag('tracking ' + dir)
      // Browser response awaited before displaying: the bubble only appears afterwards (a few ms).
      let asked = null
      const onInfo = (error, info) => {
        if (pendingInfo !== asked) return
        pendingInfo = null
        if (error !== null || !info || typeof info !== 'object') {
          // A slow answer only costs this gesture; a refusal or a missing bridge disables swipes in this document.
          if (error !== 'timeout') disabled = true
          state = 'ignore'
          spent = true
          return
        }
        can = { enabled: info.enabled === true, back: info.back === true, forward: info.forward === true }
        diag('browser: ' + JSON.stringify(can))
        if (!can.enabled) {
          if (state === 'track') {
            // Refused for this gesture only (setting, momentum, interface view).
            state = 'ignore'
            spent = true
          }
          return
        }
        if (state === 'track' && !ui) {
          try {
            ui = new Bubble(reducedMotion())
            if (!raf) raf = g.requestAnimationFrame(render)
          } catch (err) {
            diag('bubble impossible: ' + String(err)) // without a bubble, the gesture cannot succeed: we let it through
            state = 'ignore'
          }
        }
      }
      asked = {}
      pendingInfo = asked
      bridge.request('swipe:can', null, onInfo, 2000)
    }

    g.addEventListener(
      'wheel',
      (e) => {
        if (disabled || !e.isTrusted || e.ctrlKey) return // ctrl: pinch to zoom
        // Page button or path: a page that canceled the event keeps the gesture for itself.
        if (e.defaultPrevented) {
          diag('event canceled by the page (state ' + state + ')')
          if (state === 'track') {
            state = 'ignore'
            if (ui) ui.release(false, dir)
            ui = null
          }
          if (state === 'idle') state = 'ignore'
        }
        // A single re-armed timer: no clearTimeout/setTimeout on every event.
        lastWheel = now()
        if (endTimer === undefined) endTimer = g.setTimeout(onQuiet, END_GAP)
        hist.push(Math.abs(e.deltaX))
        if (hist.length > 8) hist.shift()
        dxs.push(e.deltaX)
        if (dxs.length > 8) dxs.shift()
        // During the momentum of a settled gesture: a clear rise in |deltaX| = new fingers, hence a new gesture.
        if (state === 'ignore' && spent) {
          if (!rising()) return
          spent = false
          state = 'idle'
          hist = []
          dxs = []
        }
        if (state === 'idle') {
          state = 'pending'
          accX = accY = 0
        }
        if (state === 'pending') {
          accX += e.deltaX
          accY += e.deltaY
          // A gesture start that is already decaying is the momentum of a previous gesture (page reloaded in the meantime): we leave it.
          if (Math.abs(accX) + Math.abs(accY) >= LOCK_AT && hist.length >= 3) {
            if (falling(3)) {
              state = 'ignore'
              spent = true
              return
            }
            begin(e)
          }
          if (state !== 'track') return
          progress = Math.max(0, Math.abs(accX))
        } else if (state === 'track') {
          // Positive when the finger goes in the gesture's direction.
          progress = Math.max(0, progress + (dir === 'back' ? -e.deltaX : e.deltaX))
        }
        if (state === 'track' && ui) {
          // Fingers lifted (decaying momentum) or clear overshoot: we decide right away.
          const over = can[dir] && progress >= THRESHOLD
          const forward = recentSigned() >= 0
          if (over && forward && (falling(FALL_POST) || progress >= OVERSHOOT * THRESHOLD)) return settle(true)
          if (!over && falling(FALL_PRE)) {
            // The remaining momentum may still cross the threshold: we decide on the projected position, and only if the fingers were going in the gesture's direction.
            return settle(can[dir] && forward && progress + momentum() >= THRESHOLD)
          }
          if (!raf) raf = g.requestAnimationFrame(render)
        }
      },
      { passive: true }
    )

    /** Round bubble with an arrow, in a closed shadow root. */
    class Bubble {
      constructor(reduced) {
        this.reduced = reduced
        this.reached = false
        this.placed = false
        this.host = doc.createElement('div')
        const root = this.host.attachShadow({ mode: 'closed' })
        const style = doc.createElement('style')
        style.textContent = `
          :host { all: initial; position: fixed; inset: 0; z-index: 2147483647; pointer-events: none; }
          .b { position: fixed; top: 50%; width: ${BUBBLE}px; height: ${BUBBLE}px; margin-top: -${BUBBLE / 2}px; border-radius: 50%;
            display: grid; place-items: center; box-sizing: border-box; will-change: transform, opacity;
            background: rgba(40, 40, 46, .86); color: #e8e8ed; box-shadow: 0 2px 10px rgba(0, 0, 0, .28), 0 0 0 .5px rgba(255, 255, 255, .22) inset;
            -webkit-backdrop-filter: blur(12px); backdrop-filter: blur(12px); opacity: 0; transition: none; }
          svg { width: 20px; height: 20px; transition: transform .15s, color .15s; }
          .b.reached svg { color: #0a84ff; color: AccentColor; transform: scale(1.1); }
          .b.forward svg { transform: scaleX(-1); }
          .b.forward.reached svg { transform: scaleX(-1.1) scaleY(1.1); }
        `
        this.el = doc.createElement('div')
        this.el.className = 'b'
        // No innerHTML: some pages' Trusted Types (YouTube) refuse it.
        const ns = 'http://www.w3.org/2000/svg'
        const svg = doc.createElementNS(ns, 'svg')
        const attrs = {
          viewBox: '0 0 24 24',
          fill: 'none',
          stroke: 'currentColor',
          'stroke-width': '2.4',
          'stroke-linecap': 'round',
          'stroke-linejoin': 'round'
        }
        for (const k of Object.keys(attrs)) svg.setAttribute(k, attrs[k])
        const path = doc.createElementNS(ns, 'path')
        path.setAttribute('d', 'M19 12H5M11 6l-6 6 6 6')
        svg.append(path)
        this.el.append(svg)
        root.append(style, this.el)
        ;(doc.documentElement || doc).append(this.host)
      }

      /** Position for a movement `p` (px): damped, ~24 px visible at the threshold, then strong resistance (12 px more at most). */
      update(p, d, allowed) {
        const el = this.el
        if (!this.placed) {
          this.placed = true
          el.classList.toggle('forward', d === 'forward')
          if (d === 'back') el.style.left = '0'
          else el.style.right = '0'
        }
        const t = Math.min(1, p / THRESHOLD)
        let visible // part of the bubble that is visible, from the edge
        if (!allowed) visible = 20 * (1 - Math.exp(-p / 70)) // at most half
        else if (p <= THRESHOLD) visible = SHOWN * t
        else visible = SHOWN + OVER * (1 - Math.exp(-(p - THRESHOLD) / 70))
        this.reached = allowed && p >= THRESHOLD
        el.classList.toggle('reached', this.reached)
        const sign = d === 'back' ? 1 : -1
        const x = sign * (visible - BUBBLE) // hidden off-screen at rest
        el.style.transition = 'none'
        el.style.transform = this.reduced ? 'translateX(' + sign * (SHOWN - BUBBLE) + 'px)' : 'translateX(' + x + 'px)'
        el.style.opacity = String(allowed ? 0.4 + 0.6 * t : 0.4)
      }

      /** End: exit outward if navigating, otherwise elastic return; then the bubble disappears. */
      release(navigate, d) {
        const el = this.el
        const sign = d === 'back' ? 1 : -1
        const done = () => this.host.remove()
        if (this.reduced) {
          el.style.transition = 'opacity .18s linear'
          el.style.opacity = '0'
        } else if (navigate) {
          el.style.transition = 'transform .15s ease-out, opacity .15s linear'
          el.style.transform = 'translateX(' + sign * -(BUBBLE + 4) + 'px)'
          el.style.opacity = '0'
        } else {
          // Return to the edge, without bounce (gesture with no momentum).
          el.style.transition = 'transform .3s cubic-bezier(.2,.8,.2,1), opacity .2s linear'
          el.style.transform = 'translateX(' + sign * -(BUBBLE + 4) + 'px)'
          el.style.opacity = '0'
        }
        g.setTimeout(done, 340)
      }
    }
  }
})

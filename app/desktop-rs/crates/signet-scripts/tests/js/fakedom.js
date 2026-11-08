// A very small fake DOM, enough for the page scripts to load and for swipe to run.
'use strict'

class FakeClassList {
  constructor() {
    this.set = new Set()
  }
  toggle(name, force) {
    const on = force === undefined ? !this.set.has(name) : force
    if (on) this.set.add(name)
    else this.set.delete(name)
  }
  contains(name) {
    return this.set.has(name)
  }
}

class Element {
  constructor(tag = 'div') {
    this.localName = tag
    this.id = ''
    this.style = {}
    this.classList = new FakeClassList()
    this.children = []
    this.attrs = {}
    this.listeners = {}
    this.parentElement = null
    this.scrollWidth = 0
    this.clientWidth = 0
    this.scrollLeft = 0
  }
  append(...nodes) {
    for (const n of nodes) {
      this.children.push(n)
      n.parentElement = this
    }
  }
  remove() {
    this.removed = true
    if (this.parentElement) this.parentElement.children = this.parentElement.children.filter((c) => c !== this)
  }
  attachShadow() {
    this.shadow = new Element('shadow')
    return this.shadow
  }
  setAttribute(k, v) {
    this.attrs[k] = v
  }
  addEventListener(type, fn) {
    ;(this.listeners[type] ||= []).push(fn)
  }
}

/** A window-like object with the bits swipe, gpc, identity and the guards of the form scripts need. */
function makeWindow({ href = 'https://example.com/', main = true } = {}) {
  const url = new URL(href)
  const timers = []
  const rafs = []
  const listeners = {}
  const doc = new Element('#document')
  doc.documentElement = new Element('html')
  doc.body = new Element('body')
  doc.scrollingElement = doc.documentElement
  doc.createElement = (tag) => new Element(tag)
  doc.createElementNS = (_ns, tag) => new Element(tag)
  doc.getElementById = () => null
  doc.querySelector = () => null
  doc.querySelectorAll = () => []
  const clock = { now: 1000 }
  const g = {
    Element,
    HTMLInputElement: class HTMLInputElement extends Element {},
    HTMLSelectElement: class HTMLSelectElement extends Element {},
    HTMLTextAreaElement: class HTMLTextAreaElement extends Element {},
    HTMLFormElement: class HTMLFormElement extends Element {},
    HTMLButtonElement: class HTMLButtonElement extends Element {},
    Event: class Event {},
    Date,
    console,
    location: { protocol: url.protocol, host: url.host, hash: url.hash, pathname: url.pathname, origin: url.origin, href },
    navigator: { language: 'en-US', languages: ['en-US'] },
    document: doc,
    performance: { now: () => clock.now, timeOrigin: 1700000000000, getEntriesByType: () => [], getEntriesByName: () => [] },
    matchMedia: () => ({ matches: false }),
    getComputedStyle: () => ({ overflowX: 'visible', visibility: 'visible' }),
    setTimeout: (fn, ms) => {
      timers.push({ fn, ms, due: clock.now + (ms || 0) })
      return timers.length
    },
    requestAnimationFrame: (fn) => {
      rafs.push(fn)
      return rafs.length
    },
    addEventListener: (type, fn) => {
      ;(listeners[type] ||= []).push(fn)
    },
    MutationObserver: class {
      observe() {}
    },
    innerWidth: 1000,
    innerHeight: 800
  }
  for (const proto of ['HTMLInputElement', 'HTMLSelectElement', 'HTMLTextAreaElement']) {
    Object.defineProperty(g[proto].prototype, 'value', { set() {}, get() { return '' }, configurable: true })
  }
  g.top = main ? g : {}
  g.window = g
  return {
    g,
    doc,
    listeners,
    clock,
    /** Runs the animation frames queued so far. */
    flushFrames() {
      const list = rafs.splice(0)
      list.forEach((fn) => fn(clock.now))
    },
    /** Moves the clock and runs the timers that came due. */
    advance(ms) {
      clock.now += ms
      for (let i = 0; i < timers.length; i++) {
        const t = timers[i]
        if (t && t.due <= clock.now) {
          timers[i] = null
          t.fn()
        }
      }
    },
    fire(type, event) {
      for (const fn of listeners[type] || []) fn(event)
    }
  }
}

module.exports = { Element, makeWindow }

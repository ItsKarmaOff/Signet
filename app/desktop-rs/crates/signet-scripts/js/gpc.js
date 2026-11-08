// Port of preload/gpc.ts. Defines `navigator.globalPrivacyControl` (true). All frames, main world, document start.
// Only injected while the Global Privacy Control setting is on, so there is nothing to read: when it is off the
// property is absent, like in Chrome.
(function gpc(bridge, config, g) {
  'use strict'
  g = g || globalThis
  const source = {
    get globalPrivacyControl() {
      return true
    }
  }
  const getter = Object.getOwnPropertyDescriptor(source, 'globalPrivacyControl').get
  if (!getter) return
  Object.defineProperty(g.Navigator.prototype, 'globalPrivacyControl', { get: getter, enumerable: true, configurable: true })
})

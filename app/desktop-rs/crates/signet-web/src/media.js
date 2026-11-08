(function () {
  'use strict';
  var post = window.__signetInternal;
  try { delete window.__signetInternal; } catch (e) {}
  if (typeof post !== 'function') return;

  var els = [];
  var handlers = {};
  var lastPlaying = false;
  var lastKey = '';
  var timer = 0;
  var pending = 0;
  var setI = window.setInterval.bind(window);
  var clearI = window.clearInterval.bind(window);
  var setT = window.setTimeout.bind(window);
  var stringify = JSON.stringify.bind(JSON);

  function audible(el) {
    try {
      if (el.paused || el.ended || el.muted || el.volume <= 0 || el.readyState < 2) return false;
      if (el.localName === 'audio') return true;
      if ((el.webkitAudioDecodedByteCount || 0) > 0) return true;
      var s = el.srcObject;
      return !!(s && s.getAudioTracks && s.getAudioTracks().length > 0);
    } catch (e) { return false; }
  }

  function track(el) {
    if (els.indexOf(el) < 0) els.push(el);
  }

  function anyActive() {
    for (var i = els.length - 1; i >= 0; i--) {
      var el = els[i];
      if (el.paused && !el.isConnected) { els.splice(i, 1); continue; }
      if (!el.paused && !el.ended) return true;
    }
    return false;
  }

  function primary() {
    for (var i = 0; i < els.length; i++) if (audible(els[i])) return els[i];
    return els.length ? els[els.length - 1] : null;
  }

  function abs(u) {
    try { return new URL(u, location.href).href; } catch (e) { return String(u || ''); }
  }

  function readSession() {
    var ms = navigator.mediaSession;
    var m = null, state = 'none';
    try { m = ms && ms.metadata; state = (ms && ms.playbackState) || 'none'; } catch (e) {}
    var names = Object.keys(handlers);
    if (!m && !names.length && !lastPlaying) return null;
    var art = [];
    if (m && m.artwork) {
      for (var i = 0; i < m.artwork.length; i++) {
        var a = m.artwork[i];
        art.push({ src: abs(a.src), sizes: a.sizes || '', type: a.type || '' });
      }
    }
    var el = primary();
    return {
      title: m ? String(m.title || '') : '',
      artist: m ? String(m.artist || '') : '',
      album: m ? String(m.album || '') : '',
      artwork: art,
      state: state,
      playing: lastPlaying,
      actions: names,
      duration: el && isFinite(el.duration) ? el.duration : 0,
      position: el ? el.currentTime : 0
    };
  }

  function flush() {
    pending = 0;
    patchSession();
    var playing = false;
    for (var i = 0; i < els.length; i++) if (audible(els[i])) { playing = true; break; }
    if (playing !== lastPlaying) {
      lastPlaying = playing;
      post('audio', playing ? '1' : '0');
    }
    var s = readSession();
    if (s) {
      var key = stringify([s.title, s.artist, s.album, s.artwork, s.state, s.playing, s.actions]);
      if (key !== lastKey) { lastKey = key; post('session', stringify(s)); }
    } else if (lastKey) {
      lastKey = '';
      post('session', 'null');
    }
    var active = anyActive();
    if (active && !timer) timer = setI(schedule, 1000);
    else if (!active && timer) { clearI(timer); timer = 0; }
  }

  function schedule() {
    if (!pending) pending = setT(flush, 30);
  }

  var onEvent = function (e) {
    var t = e.target;
    if (t && t.localName && (t.localName === 'audio' || t.localName === 'video') && typeof t.paused === 'boolean') {
      track(t);
      schedule();
    }
  };
  ['play', 'playing', 'pause', 'ended', 'emptied', 'volumechange', 'loadeddata', 'seeked'].forEach(function (n) {
    window.addEventListener(n, onEvent, true);
  });
  window.addEventListener('pagehide', function () {
    if (lastPlaying) { lastPlaying = false; post('audio', '0'); }
    if (lastKey) { lastKey = ''; post('session', 'null'); }
  }, true);

  // `MediaSession` is not installed yet when CEF calls OnContextCreated (it appears before the first page
  // script runs), so the patch is applied lazily: at once if possible, else on the first read of `navigator`.
  var patched = false;
  function patchSession() {
    if (patched) return true;
    try {
      var proto = window.MediaSession && window.MediaSession.prototype;
      if (!proto) return false;
      patched = true;
      ['metadata', 'playbackState'].forEach(function (name) {
        var d = Object.getOwnPropertyDescriptor(proto, name);
        if (!d || !d.set || !d.configurable) return;
        var set = d.set;
        Object.defineProperty(proto, name, {
          configurable: true, enumerable: d.enumerable, get: d.get,
          set: function (v) { var r = set.call(this, v); schedule(); return r; }
        });
      });
      var sah = proto.setActionHandler;
      if (typeof sah === 'function') {
        proto.setActionHandler = function (action, handler) {
          if (typeof handler === 'function') handlers[action] = handler; else delete handlers[action];
          schedule();
          return sah.apply(this, arguments);
        };
      }
      post('dbg', 'patched ' + location.href);
      return true;
    } catch (e) { try { post('dbg', 'patch failed: ' + e); } catch (e2) {} return true; }
  }
  if (!patchSession()) {
    try {
      var nd = Object.getOwnPropertyDescriptor(window, 'navigator');
      if (nd && nd.get && nd.configurable) {
        var nget = nd.get;
        Object.defineProperty(window, 'navigator', {
          configurable: true, enumerable: nd.enumerable, set: nd.set,
          get: function () {
            if (patchSession()) {
              try { Object.defineProperty(window, 'navigator', nd); } catch (e) {}
            }
            return nget.call(this);
          }
        });
        post('dbg', 'navigator trap installed ' + location.href);
      } else {
        post('dbg', 'no navigator trap: ' + (nd ? 'not configurable or no getter' : 'no own descriptor'));
      }
    } catch (e) { try { post('dbg', 'trap failed: ' + e); } catch (e2) {} }
  }

  function all() {
    var l = els.slice();
    try {
      var q = document.querySelectorAll('video,audio');
      for (var i = 0; i < q.length; i++) if (l.indexOf(q[i]) < 0) l.push(q[i]);
    } catch (e) {}
    return l;
  }

  function ctl(cmd) {
    try {
      if (cmd === 'toggle') cmd = lastPlaying ? 'pause' : 'play';
      var h = handlers[cmd];
      if (typeof h === 'function') { h({ action: cmd }); return true; }
      var l = all();
      var el = primary() || l[0];
      if (cmd === 'play') { if (el) el.play(); return !!el; }
      if (cmd === 'pause') { l.forEach(function (x) { x.pause(); }); return l.length > 0; }
      if (cmd === 'seekforward' && el) { el.currentTime += 10; return true; }
      if (cmd === 'seekbackward' && el) { el.currentTime -= 10; return true; }
    } catch (e) {}
    return false;
  }
  Object.defineProperty(window, '__signetMediaCtl', { value: ctl, writable: false, configurable: false, enumerable: false });
  schedule();
})();

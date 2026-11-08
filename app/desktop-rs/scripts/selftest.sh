#!/usr/bin/env bash
# Automated runtime check of the macOS bundle: serves a tiny local site, launches the app with a throwaway data
# folder and `SIGNET_SELFTEST=1`, prints the PASS/FAIL lines and always kills what it started.
#   scripts/selftest.sh [--release] [seconds]      (default: debug bundle, 150 seconds time limit)
# Windows open on the screen while it runs. Output: $WORK/run.log (WORK defaults to a new folder under $TMPDIR).
source "$(dirname "${BASH_SOURCE[0]}")/common.sh"
set +e
PROFILE=debug
[ "${1:-}" = "--release" ] && { PROFILE=release; shift; }
LIMIT="${1:-150}"
WORK="${WORK:-$(mktemp -d "${TMPDIR:-/tmp}/signet-selftest.XXXXXX")}"
PORT="${PORT:-8765}"
SITE="$WORK/site"; EXT="$WORK/ext"; DATA="$WORK/data"
mkdir -p "$SITE" "$EXT" "$DATA"

cat > "$SITE/index.html" <<'H'
<!doctype html><html><head><meta charset="utf-8"><title>Signet Test Page</title></head>
<body><h1>Signet Test Page</h1><p>Signet one</p><p>Signet two</p><p>Signet three</p><input id="t" type="text"><a href="two.html">two</a></body></html>
H
cat > "$SITE/two.html" <<'H'
<!doctype html><html><head><meta charset="utf-8"><title>Second Page</title></head><body><h1>Second Page</h1></body></html>
H
echo '{"name":"signet","list":[1,2,3],"nested":{"a":true,"b":null,"c":"text"}}' > "$SITE/data.json"
cat > "$SITE/ads.html" <<'H'
<!doctype html><html><head><meta charset="utf-8"><title>Ads</title></head><body><h1>Ads</h1>
<script src="https://pagead2.googlesyndication.com/pagead/js/adsbygoogle.js"></script>
<script src="https://securepubads.g.doubleclick.net/tag/js/gpt.js"></script>
<img src="https://googleads.g.doubleclick.net/pagead/viewthroughconversion/1/?random=1"></body></html>
H
cat > "$SITE/video.html" <<'H'
<!doctype html><html><head><meta charset="utf-8"><title>Video</title></head><body><video id="v" muted playsinline width="320" height="180"></video>
<script>
const c = document.createElement('canvas'); c.width = 320; c.height = 180; const g = c.getContext('2d');
let n = 0; setInterval(() => { g.fillStyle = 'hsl(' + (n++ * 7 % 360) + ',80%,50%)'; g.fillRect(0, 0, 320, 180); }, 50);
const v = document.getElementById('v'); v.srcObject = c.captureStream(20); v.play().catch(() => {});
</script></body></html>
H
cat > "$EXT/manifest.json" <<'H'
{"manifest_version":3,"name":"Selftest Extension","version":"1.0","description":"tiny",
 "background":{"service_worker":"bg.js"},
 "action":{"default_title":"Selftest","default_popup":"popup.html"},
 "content_scripts":[{"matches":["http://127.0.0.1/*"],"js":["cs.js"]}]}
H
cat > "$EXT/bg.js" <<'H'
console.log('BG_READY');
chrome.runtime.onMessage.addListener((m, s, send) => { console.log('BG_GOT ' + JSON.stringify(m)); send({ pong: (m && m.ping) || 0 }); return true; });
H
cat > "$EXT/cs.js" <<'H'
chrome.runtime.sendMessage({ ping: 7 }, (r) => console.log('CS_GOT ' + JSON.stringify(r)));
H
echo '<!doctype html><html><body><p>popup</p><script src="popup.js"></script></body></html>' > "$EXT/popup.html"
cat > "$EXT/popup.js" <<'H'
chrome.runtime.sendMessage({ ping: 9 }, (r) => { console.log('POPUP_GOT ' + JSON.stringify(r)); document.title = 'popup-ok'; });
H

cleanup() {
  [ -n "${SRV:-}" ] && kill "$SRV" 2>/dev/null
  pkill -f "${DIST:-$ROOT/dist}/Signet.app" 2>/dev/null
  true
}
trap cleanup EXIT
python3 -m http.server "$PORT" --bind 127.0.0.1 -d "$SITE" >"$WORK/http.log" 2>&1 &
SRV=$!
sleep 1

DIST="${SIGNET_DIST:-$ROOT/dist}"
BIN="$DIST/Signet.app/Contents/MacOS/signet"
SIGNET_NO_KEYCHAIN=1 SIGNET_LINK_OFF=1 SIGNET_DEBUG=1 SIGNET_WEB_DEBUG=1 SIGNET_SELFTEST=1 SIGNET_DATA_DIR="$DATA" SIGNET_AUTOQUIT="$LIMIT" \
  SIGNET_TEST_ONLY="${ONLY:-${SIGNET_TEST_ONLY:-}}" SIGNET_TEST_BASE="http://127.0.0.1:$PORT" SIGNET_TEST_EXT="$EXT" \
  timeout $((LIMIT + 30)) "$BIN" >"$WORK/run.log" 2>&1
CODE=$?
echo "exit code: $CODE" | tee -a "$WORK/run.log"
sleep 2
LEFT=$(pgrep -f "Contents/Frameworks/Signet Helper" | wc -l | tr -d ' ')
echo "helpers left after exit: $LEFT" | tee -a "$WORK/run.log"
grep -E "^\[selftest" "$WORK/run.log"
echo "log: $WORK/run.log"

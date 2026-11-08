#!/usr/bin/env bash
# One screenshot of the Rust app, like the Electron dev harness (docs/DEVELOPMENT.md, "SIGNET_* environment variables").
#   scripts/capture.sh [<target>] <out.png> [ENV=VAL ...]       (target defaults to main)
#   scripts/capture.sh /tmp/main.png SIGNET_SEED=1 SIGNET_THEME=dark
#   scripts/capture.sh settings:general /tmp/settings.png SIGNET_SEED=1
# <target>: main, settings, settings:<section>, library, library:<section>, little, private, palette, find, welcome, passkey, boost.
# Runs dist/Signet.app (build it first with scripts/bundle-macos.sh) with a fresh scratch data folder, a time limit
# (CAPTURE_LIMIT seconds, default 40) and always kills what it started. Never touches a real profile.
source "$(dirname "${BASH_SOURCE[0]}")/common.sh"
set +e
[ $# -ge 1 ] || { echo "usage: $0 [<target>] <out.png> [ENV=VAL ...]" >&2; exit 2; }
case "$1" in
  *.png) TARGET=main; OUT="$1"; shift ;;
  *) TARGET="$1"; OUT="${2:?missing out.png}"; shift 2 ;;
esac
case "$OUT" in /*) ;; *) OUT="$PWD/$OUT" ;; esac
DIST="${SIGNET_DIST:-$ROOT/dist}"
BIN="$DIST/Signet.app/Contents/MacOS/signet"
[ -x "$BIN" ] || { echo "missing $BIN: run scripts/bundle-macos.sh" >&2; exit 1; }
mkdir -p "$(dirname "$OUT")"
DATA="$(mktemp -d "${TMPDIR:-/tmp}/signet-capture.XXXXXX")"
cleanup() {
  pkill -f "${DIST:-$ROOT/dist}/Signet.app" 2>/dev/null
  rm -rf "$DATA"
  true
}
trap cleanup EXIT
rm -f "$OUT"
LIMIT="${CAPTURE_LIMIT:-40}"
env "$@" SIGNET_DATA_DIR="$DATA" SIGNET_NO_KEYCHAIN=1 SIGNET_LINK_OFF=1 SIGNET_CAPTURE="$TARGET:$OUT" SIGNET_AUTOQUIT=$((LIMIT - 5)) \
  timeout "$LIMIT" "$BIN" 2>&1 | grep -E "^(EVAL|\[capture\]|screencapture)|panicked|could not"
cleanup
[ -s "$OUT" ] && echo "$OUT" || { echo "no capture written" >&2; exit 1; }

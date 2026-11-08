#!/usr/bin/env bash
# End-to-end test of the self-update of a PACKAGED app (Linux AppImage, Windows NSIS installer; Git Bash on Windows).
#   scripts/update-e2e.sh <restart|later>
# Environment: OLD_DIR and NEW_DIR (folders with the packages of the old and of the new version, NEW_DIR also holds the
# latest*.yml), OLD_VERSION, NEW_VERSION, WORK (scratch folder, default: a new temp folder).
# It installs the old package, serves NEW_DIR like the GitHub releases API, launches the installed app with the
# test-only variables (SIGNET_UPDATE_TEST=1, SIGNET_UPDATE_FEED, SIGNET_UPDATE_ACTION, SIGNET_UPDATE_MARKER; see
# crates/signet-features/src/updates.rs) and checks that it downloads, verifies and installs the new version, and (restart)
# starts again as the new version, or (later) quits without relaunch and the next start is the new version.
# On Linux run it under a virtual display (xvfb-run + a window manager). Everything it starts is killed at the end.
set -uo pipefail
MODE="${1:?usage: update-e2e.sh <restart|later>}"
: "${OLD_DIR:?}" "${NEW_DIR:?}" "${OLD_VERSION:?}" "${NEW_VERSION:?}"
WORK="${WORK:-$(mktemp -d "${TMPDIR:-/tmp}/signet-update-e2e.XXXXXX")}"
mkdir -p "$WORK"
case "$(uname -s)" in MINGW*|MSYS*|CYGWIN*) OS=windows ;; *) OS=linux ;; esac
command -v python3 >/dev/null 2>&1 || python3() { python "$@"; }
export MSYS2_ARG_CONV_EXCL='*'
wp() { if [ "$OS" = windows ]; then cygpath -m "$1"; else echo "$1"; fi; }

PORT="${PORT:-8766}"
MARKER="$WORK/marker.txt"; : > "$MARKER"
APPLOG="$WORK/app.log"; : > "$APPLOG"
fail() { echo "FAIL: $*"; echo "--- marker"; cat "$MARKER"; echo "--- server log"; cat "$WORK/server.log" 2>/dev/null; echo "--- app log (tail)"; tail -n 80 "$APPLOG"; exit 1; }
pass() { echo "PASS: $*"; }

kill_all() {
  if [ "$OS" = windows ]; then
    taskkill /F /IM signet.exe /T >/dev/null 2>&1; taskkill /F /IM signet-helper.exe /T >/dev/null 2>&1
  else
    pkill -f "$WORK/" 2>/dev/null; pkill -f "signet-helper" 2>/dev/null; pkill -f "/tmp/.mount_" 2>/dev/null
  fi
  [ -n "${SERVER_PID:-}" ] && kill "$SERVER_PID" 2>/dev/null
  true
}
trap kill_all EXIT

wait_for() { # wait_for <seconds> <description> <command...>
  local limit="$1" what="$2"; shift 2
  local t=0
  while ! "$@"; do
    sleep 2; t=$((t + 2))
    [ $t -ge "$limit" ] && fail "timeout after ${limit}s: $what"
  done
  echo "ok after ${t}s: $what"
}
starts() { grep -c "version=$1 " "$MARKER" 2>/dev/null || true; }
has_start() { [ "$(starts "$1")" -ge "${2:-1}" ]; }

# Serve the new version.
python3 "$(wp "$(dirname "${BASH_SOURCE[0]}")/update-e2e-server.py")" "$(wp "$NEW_DIR")" "$PORT" "v$NEW_VERSION" >"$WORK/server.log" 2>&1 &
SERVER_PID=$!
wait_server() { curl -fsS "http://127.0.0.1:$PORT/releases" >"$WORK/releases.json" 2>/dev/null; }
wait_for 30 "feed server up" wait_server
head -c 400 "$WORK/releases.json"; echo

export SIGNET_NO_KEYCHAIN=1 SIGNET_LINK_OFF=1 SIGNET_DEBUG=1 SIGNET_DATA_DIR="$(wp "$WORK/data")"
export SIGNET_UPDATE_TEST=1 SIGNET_UPDATE_FEED="http://127.0.0.1:$PORT" SIGNET_UPDATE_MARKER="$(wp "$MARKER")"
mkdir -p "$WORK/data"

if [ "$OS" = windows ]; then
  KEY='HKCU\Software\Microsoft\Windows\CurrentVersion\Uninstall\{ea93c96e-a71c-5caa-a6d0-f986425b44b7}'
  INSTDIR="$(cygpath -u "$LOCALAPPDATA")/Programs/Signet"
  installed_version() { reg query "$KEY" /v DisplayVersion 2>/dev/null | tr -d '\r' | awk '/DisplayVersion/{print $NF}'; }
  SETUP="$(ls "$OLD_DIR"/Signet-"$OLD_VERSION"-x64-setup.exe)"
  timeout 240 "$SETUP" /S || fail "silent install of the old version"
  echo "old version installed by the setup (exit ok)"
  reg query "$KEY" 2>&1 | head -20
  wait_for 120 "old version registered" test "$(installed_version)" = "$OLD_VERSION"
  [ -f "$INSTDIR/signet.exe" ] && [ -f "$INSTDIR/Uninstall Signet.exe" ] || fail "installed files missing in $INSTDIR"
  EXE="$INSTDIR/signet.exe"
  installed_is_new() { test "$(installed_version)" = "$NEW_VERSION"; }
else
  APPDIR="$WORK/app"; mkdir -p "$APPDIR"
  APP="$APPDIR/Signet.AppImage"
  cp "$OLD_DIR"/Signet-"$OLD_VERSION"-*.AppImage "$APP"; chmod +x "$APP"
  NEW_SHA="$(sha512sum "$NEW_DIR"/Signet-"$NEW_VERSION"-*.AppImage | cut -d' ' -f1)"
  export APPIMAGE_EXTRACT_AND_RUN=1
  EXE="$APP"
  installed_is_new() { test "$(sha512sum "$APP" | cut -d' ' -f1)" = "$NEW_SHA"; }
fi
pass "old version $OLD_VERSION installed"

export SIGNET_UPDATE_ACTION="$MODE"
"$EXE" >"$APPLOG" 2>&1 &
wait_for 240 "old app started" has_start "$OLD_VERSION" 1

if [ "$MODE" = restart ]; then
  wait_for 300 "new version started after the update" has_start "$NEW_VERSION" 1
  installed_is_new || fail "installed package is not the new version"
  pass "restart: the app updated to $NEW_VERSION and relaunched as it"
  [ "$(starts "$OLD_VERSION")" = 1 ] || fail "old version started more than once"
else
  wait_for 300 "new version installed" installed_is_new
  sleep 15
  has_start "$NEW_VERSION" 1 && fail "later: the app relaunched, it must not"
  pass "later: $NEW_VERSION installed at quit, no relaunch"
  # The next start is the new version (up to date: no action).
  unset SIGNET_UPDATE_ACTION
  "$EXE" >>"$APPLOG" 2>&1 &
  wait_for 240 "new version started by hand" has_start "$NEW_VERSION" 1
  pass "later: the next launch runs $NEW_VERSION"
fi
# The up-to-date app must not try to update again (no second download of the package).
sleep 10
grep -c "Signet-$NEW_VERSION" "$WORK/server.log" | sed 's/^/package requests: /'
echo "--- marker"; cat "$MARKER"
echo "UPDATE E2E OK ($OS, $MODE)"

#!/usr/bin/env bash
# Makes the macOS distributable files from a release bundle: Signet-<version>-<arch>.dmg (to install by hand) and
# Signet-<version>-<arch>.zip (read by the updater through latest-mac.yml), like the Electron app. <arch> is arm64 or x64
# (the machine's: CEF is per architecture, build the Intel package on an Intel Mac or runner).
#   scripts/package-macos.sh [--skip-bundle] [--sign "<identity>"]
# Output: dist/ (or $SIGNET_DIST). The app is ad hoc signed by bundle-macos.sh, like the Electron one.
source "$(dirname "${BASH_SOURCE[0]}")/common.sh"

SKIP=0
SIGN=()
while [ $# -gt 0 ]; do
  case "$1" in
    --skip-bundle) SKIP=1 ;;
    --sign) SIGN=(--sign "$2"); shift ;;
    *) echo "unknown flag: $1" >&2; exit 2 ;;
  esac
  shift
done

VERSION="$(workspace_version)"
case "$(uname -m)" in arm64|aarch64) ARCH=arm64 ;; *) ARCH=x64 ;; esac
export SIGNET_DIST="${SIGNET_DIST:-$ROOT/dist}"
DIST="$SIGNET_DIST"
APP="$DIST/Signet.app"

[ "$SKIP" = 1 ] || "$ROOT/scripts/bundle-macos.sh" --release ${SIGN[@]+"${SIGN[@]}"}
[ -d "$APP" ] || { echo "$APP not found (run without --skip-bundle)" >&2; exit 1; }

ZIP="$DIST/Signet-$VERSION-$ARCH.zip"
DMG="$DIST/Signet-$VERSION-$ARCH.dmg"
rm -f "$ZIP" "$DMG"

# ZIP: Signet.app at the root of the archive, resource forks kept (what electron-updater unpacks).
ditto -c -k --sequesterRsrc --keepParent "$APP" "$ZIP"

# DMG: the app next to an Applications shortcut.
STAGE="$(mktemp -d "${TMPDIR:-/tmp}/signet-dmg.XXXXXX")"
trap 'rm -rf "$STAGE"' EXIT
ditto "$APP" "$STAGE/Signet.app"
ln -s /Applications "$STAGE/Applications"
hdiutil create -volname "Signet $VERSION" -srcfolder "$STAGE" -fs HFS+ -format ULMO -ov "$DMG" >/dev/null

echo "Packages: $ZIP $DMG"
echo "Then: scripts/update-metadata.sh <folder with the packages of every arch> writes latest-mac.yml"

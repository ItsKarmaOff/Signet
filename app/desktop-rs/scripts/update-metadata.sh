#!/usr/bin/env bash
# Writes the electron-updater metadata files (latest.yml, latest-linux.yml, latest-linux-arm64.yml, latest-mac.yml) for the
# packages found in a folder, in the exact format electron-builder emits. Packages that are missing are left out, and a file
# with no package at all is not written, so it works for one system as well as for the merged output of a whole release.
#   scripts/update-metadata.sh <folder> [version]
# The version defaults to [workspace.package] in Cargo.toml. No .blockmap is produced (differential download is off:
# electron-updater falls back to a full download).
source "$(dirname "${BASH_SOURCE[0]}")/common.sh"

DIR="${1:?usage: update-metadata.sh <folder> [version]}"
VERSION="${2:-$(workspace_version)}"
DATE="$(date -u +%Y-%m-%dT%H:%M:%S.000Z)"

sha512_b64() { openssl dgst -sha512 -binary "$1" | openssl base64 -A; }
size_of() { wc -c < "$1" | tr -d ' '; }

# yml <output name> <package file>...   (the first existing package is also the top-level `path`)
yml() {
  local out="$1"; shift
  local files=() f
  for f in "$@"; do [ -f "$DIR/$f" ] && files+=("$f"); done
  [ ${#files[@]} -gt 0 ] || return 0
  {
    echo "version: $VERSION"
    echo "files:"
    for f in "${files[@]}"; do
      echo "  - url: $f"
      echo "    sha512: $(sha512_b64 "$DIR/$f")"
      echo "    size: $(size_of "$DIR/$f")"
    done
    echo "path: ${files[0]}"
    echo "sha512: $(sha512_b64 "$DIR/${files[0]}")"
    echo "releaseDate: '$DATE'"
  } > "$DIR/$out"
  echo "Wrote $DIR/$out (${#files[@]} files)"
}

yml latest.yml "Signet-$VERSION-x64-setup.exe"
yml latest-linux.yml "Signet-$VERSION-x86_64.AppImage" "Signet-$VERSION-amd64.deb" "Signet-$VERSION-x86_64.rpm"
yml latest-linux-arm64.yml "Signet-$VERSION-arm64.AppImage" "Signet-$VERSION-arm64.deb" "Signet-$VERSION-aarch64.rpm"
yml latest-mac.yml "Signet-$VERSION-x64.zip" "Signet-$VERSION-arm64.zip" "Signet-$VERSION-x64.dmg" "Signet-$VERSION-arm64.dmg"

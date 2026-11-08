#!/usr/bin/env bash
# Build Signet and lay out dist/signet-linux/ (binary, CEF runtime, desktop file, icons). Linux only, untested so far.
#   scripts/bundle-linux.sh [--release|--debug] [--tar]
# Debug is the default; --release strips symbols. --tar also writes dist/signet-<version>-linux-<arch>.tar.gz.
#
# The CEF distribution is the one cef-dll-sys downloads during `cargo build` into $CEF_PATH
# (default: app/desktop-rs/.cache/cef/<cef version>/cef_linux_<arch>). Set CEF_PATH to reuse another cache.
#
# Layout (everything sits in one folder, the loader finds libcef.so through an rpath of $ORIGIN):
#   signet              launcher script (forces X11/XWayland, then runs signet-bin)
#   signet-bin          the app
#   signet-helper       CEF subprocess (kept for completeness, the app can also re-run itself)
#   libcef.so, libEGL.so, libGLESv2.so, libvk_swiftshader.so, libvulkan.so.1, vk_swiftshader_icd.json,
#   *.pak, icudtl.dat, v8_context_snapshot.bin, locales/, chrome-sandbox   (from the CEF distribution)
#   signet.desktop, signet.png, icons/hicolor/<size>/apps/signet.png
#
# chrome-sandbox: the app starts CEF with no_sandbox, so the SUID helper is NOT needed and is not made SUID here.
# If the sandbox is enabled one day: sudo chown root:root chrome-sandbox && sudo chmod 4755 chrome-sandbox.
#
# Packaging notes:
#  - tar.gz: unpack anywhere, run ./signet.
#  - deb/rpm: install the folder to /opt/signet, symlink /usr/bin/signet -> /opt/signet/signet, put signet.desktop in
#    /usr/share/applications (Exec=signet %U, Icon=signet) and icons/hicolor in /usr/share/icons. Tools: fpm or cargo-deb.
#  - AppImage: copy the folder to AppDir/usr/bin, AppDir/AppRun = a link to usr/bin/signet, signet.desktop and signet.png at
#    the AppDir root, then run appimagetool. Bundle libs the target machines may lack (libxkbcommon, libvulkan) only if needed.
source "$(dirname "${BASH_SOURCE[0]}")/common.sh"

[ "$(uname -s)" = Linux ] || { echo "bundle-linux.sh only runs on Linux" >&2; exit 1; }

PROFILE=debug
MAKE_TAR=0
for a in "$@"; do
  case "$a" in
    --release) PROFILE=release ;;
    --debug) PROFILE=debug ;;
    --tar) MAKE_TAR=1 ;;
    *) echo "unknown flag: $a" >&2; exit 2 ;;
  esac
done

VERSION="$(workspace_version)"
ARCH="$(uname -m)"
ICON_SRC="$ROOT/../desktop/resources/icons/aurore.png"
DIST="$ROOT/dist"
OUT="$DIST/signet-linux"

cd "$ROOT"
# rpath $ORIGIN: patchelf after the build when available, else a link argument (rebuilds once with other flags).
if ! command -v patchelf >/dev/null 2>&1; then
  export RUSTFLAGS="${RUSTFLAGS:-} -C link-arg=-Wl,-rpath,\$ORIGIN"
fi
if [ "$PROFILE" = release ]; then
  cargo build --release -p signet-app -p signet-helper
else
  cargo build -p signet-app -p signet-helper
fi
TARGET="$ROOT/target/$PROFILE"

CEF_DIR="$(find_cef_linux)" || { echo "CEF (cef_linux_*) not found under $CEF_PATH; run the cargo build first" >&2; exit 1; }

# Only the folder we own is removed.
[ -d "$OUT" ] && rm -r "$OUT"
mkdir -p "$OUT"

install -m 755 "$TARGET/signet" "$OUT/signet-bin"
install -m 755 "$TARGET/signet-helper" "$OUT/signet-helper"
if command -v patchelf >/dev/null 2>&1; then
  patchelf --set-rpath '$ORIGIN' "$OUT/signet-bin" "$OUT/signet-helper"
fi
if [ "$PROFILE" = release ]; then
  strip --strip-unneeded "$OUT/signet-bin" "$OUT/signet-helper"
fi

# CEF runtime: everything except the SDK (headers, cmake, wrapper sources) and static libraries.
for f in "$CEF_DIR"/*; do
  case "$(basename "$f")" in
    include|cmake|libcef_dll|CMakeLists.txt|archive.json|CREDITS.html|README.txt|*.a|*.lib) continue ;;
  esac
  cp -R "$f" "$OUT/"
done
# The sandbox helper stays non-SUID (see header); make sure no stray SUID bit survives.
[ -f "$OUT/chrome-sandbox" ] && chmod 755 "$OUT/chrome-sandbox"

# Launcher: Wayland sessions have no child windows for CEF, so run under XWayland when an X server exists.
cat > "$OUT/signet" <<'LAUNCH'
#!/usr/bin/env sh
here="$(dirname "$(readlink -f "$0")")"
if [ -n "${WAYLAND_DISPLAY:-}" ] && [ -n "${DISPLAY:-}" ]; then
  unset WAYLAND_DISPLAY
  export XDG_SESSION_TYPE=x11
fi
export LD_LIBRARY_PATH="$here${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
exec "$here/signet-bin" "$@"
LAUNCH
chmod 755 "$OUT/signet"

cat > "$OUT/signet.desktop" <<'DESKTOP'
[Desktop Entry]
Type=Application
Name=Signet
GenericName=Web Browser
Comment=Browse the web with spaces, folders and split views
Exec=signet %U
Icon=signet
Terminal=false
Categories=Network;WebBrowser;
MimeType=text/html;application/xhtml+xml;x-scheme-handler/http;x-scheme-handler/https;x-scheme-handler/signet;
StartupNotify=true
StartupWMClass=signet
DESKTOP

# Icons: the 1024 px PNG as is, plus the usual sizes when ImageMagick is there.
if [ -f "$ICON_SRC" ]; then
  cp "$ICON_SRC" "$OUT/signet.png"
  mkdir -p "$OUT/icons/hicolor/1024x1024/apps"
  cp "$ICON_SRC" "$OUT/icons/hicolor/1024x1024/apps/signet.png"
  IM=""
  command -v magick >/dev/null 2>&1 && IM="magick"
  [ -z "$IM" ] && command -v convert >/dev/null 2>&1 && IM="convert"
  if [ -n "$IM" ]; then
    for s in 16 32 48 64 128 256 512; do
      mkdir -p "$OUT/icons/hicolor/${s}x${s}/apps"
      $IM "$ICON_SRC" -resize "${s}x${s}" "$OUT/icons/hicolor/${s}x${s}/apps/signet.png"
    done
  fi
else
  echo "warning: icon not found: $ICON_SRC" >&2
fi

if command -v desktop-file-validate >/dev/null 2>&1; then
  desktop-file-validate "$OUT/signet.desktop" || echo "warning: signet.desktop has issues" >&2
fi

echo "Bundled $OUT ($PROFILE, version $VERSION). Run: $OUT/signet"

if [ "$MAKE_TAR" = 1 ]; then
  TARBALL="$DIST/signet-$VERSION-linux-$ARCH.tar.gz"
  tar -C "$DIST" -czf "$TARBALL" signet-linux
  echo "Archive $TARBALL"
fi

#!/usr/bin/env bash
# Build Signet and assemble dist/Signet.app (CEF framework + helper apps).
#   scripts/bundle-macos.sh [--release|--debug] [--sign "<identity>"]
# Debug is the default; --release also strips symbols. Without --sign the bundle is ad hoc signed (local use).
# With --sign "Developer ID Application: Name (TEAMID)" every code part is signed with the hardened runtime and
# scripts/macos/entitlements.plist, ready for notarization (see TESTING.md and README.md, "Signing and notarizing").
# The CEF distribution is downloaded once by the cef-dll-sys build script into $CEF_PATH
# (default: app/desktop-rs/.cache/cef) and reused afterwards.
source "$(dirname "${BASH_SOURCE[0]}")/common.sh"

PROFILE=debug
SIGN_ID=""
while [ $# -gt 0 ]; do
  case "$1" in
    --release) PROFILE=release ;;
    --debug) PROFILE=debug ;;
    --sign)
      [ $# -ge 2 ] || { echo "--sign needs an identity" >&2; exit 2; }
      SIGN_ID="$2"; shift ;;
    *) echo "unknown flag: $1" >&2; exit 2 ;;
  esac
  shift
done

APP_NAME="Signet"
APP_ID="dev.signet.app.gpui"
# Version of [workspace.package] in Cargo.toml.
VERSION="$(awk '/^\[/{s=($0=="[workspace.package]")} s && /^version *=/{gsub(/[" ]/,"",$0); sub(/^version=/,"",$0); print; exit}' "$ROOT/Cargo.toml")"
VERSION="${VERSION:-0.5.0}"
ICON="$ROOT/../desktop/build/icon.icns"
# SIGNET_DIST: output folder (default dist/), so parallel agents can each bundle their own copy.
DIST="${SIGNET_DIST:-$ROOT/dist}"
APP="$DIST/$APP_NAME.app"

cd "$ROOT"
if [ "$PROFILE" = release ]; then
  cargo build --release -p signet-app -p signet-helper
else
  cargo build -p signet-app -p signet-helper
fi
TARGET="$ROOT/target/$PROFILE"

FRAMEWORK="$(find_cef_framework)" || { echo "CEF framework not found under $CEF_PATH" >&2; exit 1; }

# Only the folders we own are removed.
[ -d "$APP" ] && rm -r "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources" "$APP/Contents/Frameworks"

# Keys only the main app needs (the helper apps are background-only).
main_keys() {
  cat <<MAIN
  <key>CFBundleIconFile</key><string>icon</string>
  <key>NSPrincipalClass</key><string>NSApplication</string>
  <key>LSApplicationCategoryType</key><string>public.app-category.productivity</string>
  <key>NSCameraUsageDescription</key><string>Websites can ask to use the camera.</string>
  <key>NSMicrophoneUsageDescription</key><string>Websites can ask to use the microphone.</string>
  <key>NSLocationWhenInUseUsageDescription</key><string>Websites can ask for your location.</string>
  <key>NSBluetoothAlwaysUsageDescription</key><string>Signet looks for your phone over Bluetooth to sign in with a passkey.</string>
  <key>NSLocalNetworkUsageDescription</key><string>Signet talks to your phone on your local network to send it pages and show its dev servers.</string>
  <key>NSBonjourServices</key>
  <array>
    <string>_signet._tcp</string>
  </array>
  <key>CFBundleURLTypes</key>
  <array>
    <dict>
      <key>CFBundleURLName</key><string>Web</string>
      <key>CFBundleTypeRole</key><string>Viewer</string>
      <key>CFBundleURLSchemes</key>
      <array><string>http</string><string>https</string></array>
    </dict>
    <dict>
      <key>CFBundleURLName</key><string>Signet App</string>
      <key>CFBundleTypeRole</key><string>Viewer</string>
      <key>CFBundleURLSchemes</key>
      <array><string>signet</string></array>
    </dict>
  </array>
  <key>CFBundleDocumentTypes</key>
  <array>
    <dict>
      <key>CFBundleTypeName</key><string>HTML document</string>
      <key>CFBundleTypeRole</key><string>Viewer</string>
      <key>LSHandlerRank</key><string>Default</string>
      <key>LSItemContentTypes</key>
      <array><string>public.html</string></array>
      <key>CFBundleTypeExtensions</key>
      <array><string>html</string><string>htm</string></array>
    </dict>
    <dict>
      <key>CFBundleTypeName</key><string>XHTML document</string>
      <key>CFBundleTypeRole</key><string>Viewer</string>
      <key>LSHandlerRank</key><string>Default</string>
      <key>LSItemContentTypes</key>
      <array><string>public.xhtml</string></array>
      <key>CFBundleTypeExtensions</key>
      <array><string>xhtml</string><string>xht</string></array>
    </dict>
  </array>
MAIN
}

plist() { # plist <path> <executable> <bundle id> <display name> <ui element 0|1>
  cat > "$1" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleDevelopmentRegion</key><string>en</string>
  <key>CFBundleExecutable</key><string>$2</string>
  <key>CFBundleIdentifier</key><string>$3</string>
  <key>CFBundleInfoDictionaryVersion</key><string>6.0</string>
  <key>CFBundleName</key><string>$4</string>
  <key>CFBundleDisplayName</key><string>$4</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>$VERSION</string>
  <key>CFBundleVersion</key><string>$VERSION</string>
  <key>CFBundleSignature</key><string>????</string>
  <key>LSMinimumSystemVersion</key><string>11.0</string>
  <key>LSEnvironment</key><dict><key>MallocNanoZone</key><string>0</string></dict>
  <key>NSSupportsAutomaticGraphicsSwitching</key><true/>
  <key>NSHighResolutionCapable</key><true/>
$( if [ "$5" = 1 ]; then echo '  <key>LSUIElement</key><string>1</string>'; else main_keys; fi )
</dict>
</plist>
PLIST
  plutil -lint "$1" >/dev/null
}

plist "$APP/Contents/Info.plist" "signet" "$APP_ID" "$APP_NAME" 0
cp "$TARGET/signet" "$APP/Contents/MacOS/signet"
[ -f "$ICON" ] && cp "$ICON" "$APP/Contents/Resources/icon.icns"

# ditto keeps symlinks and attributes of the framework.
ditto "$FRAMEWORK" "$APP/Contents/Frameworks/Chromium Embedded Framework.framework"

for kind in "" " (GPU)" " (Renderer)" " (Plugin)" " (Alerts)"; do
  name="$APP_NAME Helper$kind"
  suffix="$(echo "$kind" | tr -d ' ()' | tr 'A-Z' 'a-z')"
  id="$APP_ID.helper${suffix:+.$suffix}"
  hdir="$APP/Contents/Frameworks/$name.app/Contents"
  mkdir -p "$hdir/MacOS"
  plist "$hdir/Info.plist" "$name" "$id" "$name" 1
  cp "$TARGET/signet-helper" "$hdir/MacOS/$name"
done

# Release: drop local symbols (the bundle is much smaller). Done before signing, which seals the final bytes.
if [ "$PROFILE" = release ]; then
  strip -x "$APP/Contents/MacOS/signet"
  for h in "$APP"/Contents/Frameworks/*Helper*.app; do strip -x "$h/Contents/MacOS/"*; done
  # The app only sets en-US and fr-FR (main.rs); the other locale folders of the framework are optional (CEF falls back to en).
  for d in "$APP/Contents/Frameworks/Chromium Embedded Framework.framework/Resources/"*.lproj; do
    case "$(basename "$d")" in en*|fr*) ;; *) rm -rf "$d" ;; esac
  done
fi

# Signing goes inside out (framework, helpers, app), never with --deep.
ENTITLEMENTS="$ROOT/scripts/macos/entitlements.plist"
if [ -n "$SIGN_ID" ]; then
  # Hardened runtime + secure timestamp (required by notarization). Executables get the entitlements, the framework does not.
  sign_lib() { codesign --force --timestamp --options runtime --sign "$SIGN_ID" "$1" >/dev/null; }
  sign_exe() { codesign --force --timestamp --options runtime --entitlements "$ENTITLEMENTS" --sign "$SIGN_ID" "$1" >/dev/null; }
else
  sign_lib() { codesign --force --sign - "$1" >/dev/null; }
  sign_exe() { codesign --force --sign - "$1" >/dev/null; }
fi
sign_lib "$APP/Contents/Frameworks/Chromium Embedded Framework.framework"
for h in "$APP"/Contents/Frameworks/*Helper*.app; do sign_exe "$h"; done
sign_exe "$APP"
codesign --verify --strict "$APP"

echo "Bundled $APP ($PROFILE, $( [ -n "$SIGN_ID" ] && echo "signed: $SIGN_ID" || echo "ad hoc signed" ), version $VERSION)"
if [ -n "$SIGN_ID" ]; then
  echo "Notarize: ditto -c -k --keepParent \"$APP\" dist/Signet.zip && xcrun notarytool submit dist/Signet.zip --keychain-profile <profile> --wait && xcrun stapler staple \"$APP\""
fi

#!/usr/bin/env bash
# Builds the app in Release and installs it on the connected iPhone (signed with the team from app.json).
# Usage: npm run mobile:device (from the root). The JS is bundled: the app works without the Mac.
# With a free Apple account, the app expires after 7 days: re-run this command.
set -euo pipefail
cd "$(dirname "$0")/.."

# First physical iPhone connected (or the one given by IOS_DEVICE).
device="${IOS_DEVICE:-}"
if [ -z "$device" ]; then
  json="$(mktemp)"
  xcrun devicectl list devices --json-output "$json" >/dev/null
  device="$(node -e '
    const d = JSON.parse(require("fs").readFileSync(process.argv[1], "utf8")).result.devices
    const phone = d.find((x) => x.hardwareProperties?.reality === "physical" && x.hardwareProperties?.platform === "iOS" && x.connectionProperties?.pairingState === "paired")
    if (phone) console.log(phone.hardwareProperties.udid)
  ' "$json")"
  rm -f "$json"
fi
[ -n "$device" ] || { echo "No iPhone connected and paired." >&2; exit 1; }

# Up-to-date native project (generated ios/ folder, ignored by git), without touching the package.json scripts.
cp package.json /tmp/signet-mobile-package.json
CI=1 npx expo prebuild --platform ios
cp /tmp/signet-mobile-package.json package.json

# -allowProvisioningUpdates: Xcode creates the signing profile if missing (expo run does not allow it).
xcodebuild -workspace ios/Signet.xcworkspace -scheme Signet -configuration Release \
  -destination "id=$device" -allowProvisioningUpdates -derivedDataPath ios/build -quiet build
xcrun devicectl device install app --device "$device" ios/build/Build/Products/Release-iphoneos/Signet.app
echo "Installed on the iPhone ($device)."

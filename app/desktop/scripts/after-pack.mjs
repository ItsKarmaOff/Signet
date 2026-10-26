// electron-builder, after the Linux app is assembled: the executable becomes a small launcher script.
//
// Under Wayland, Signet must run under XWayland: Wayland forbids an app from placing its windows and keeping them
// on top, and the mini player would not work there. The `--ozone-platform=x11` option must be there from
// process start (Electron picks the platform before our code runs). Relaunching from the app does not work
// with an AppImage: Chromium starts the new process without elevation rights, and the FUSE mount fails.
// A `--ozone-platform=…` passed by hand takes precedence (e.g. `--ozone-platform=wayland`).
import { chmod, rename, writeFile } from 'node:fs/promises'
import { join } from 'node:path'

const launcher = (bin) => `#!/bin/sh
# Signet: under Wayland, start under XWayland (see scripts/after-pack.mjs).
HERE=$(dirname "$(readlink -f "$0")")
for arg in "$@"; do
  case "$arg" in
    --ozone-platform | --ozone-platform=*) exec "$HERE/${bin}" "$@" ;;
  esac
done
if [ -n "$DISPLAY" ] && { [ -n "$WAYLAND_DISPLAY" ] || [ "$XDG_SESSION_TYPE" = wayland ]; }; then
  exec "$HERE/${bin}" --ozone-platform=x11 "$@"
fi
exec "$HERE/${bin}" "$@"
`

export default async function afterPack(context) {
  if (context.electronPlatformName !== 'linux') return
  const name = context.packager.executableName
  const bin = `${name}-bin`
  await rename(join(context.appOutDir, name), join(context.appOutDir, bin))
  await writeFile(join(context.appOutDir, name), launcher(bin))
  await chmod(join(context.appOutDir, name), 0o755)
}

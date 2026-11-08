#!/usr/bin/env bash
# Bundle then launch Signet. Flags before `--` go to the bundler, the rest goes to the app.
#   scripts/run-macos.sh [--release] [-- app args...]
set -euo pipefail
DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
BUNDLE_ARGS=()
APP_ARGS=()
seen_sep=0
for a in "$@"; do
  if [ "$seen_sep" = 1 ]; then APP_ARGS+=("$a")
  elif [ "$a" = "--" ]; then seen_sep=1
  else BUNDLE_ARGS+=("$a"); fi
done
"$DIR/bundle-macos.sh" ${BUNDLE_ARGS[@]+"${BUNDLE_ARGS[@]}"}
# Run the executable directly so stdout/stderr stay attached to the terminal.
exec "$DIR/../dist/Signet.app/Contents/MacOS/signet" ${APP_ARGS[@]+"${APP_ARGS[@]}"}

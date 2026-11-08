#!/usr/bin/env bash
# Shared helpers for the build scripts. Source it, do not run it.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
# The CEF distribution (about 500 MB) is cached here and reused by every build. Override with CEF_PATH.
export CEF_PATH="${CEF_PATH:-$ROOT/.cache/cef}"
export CARGO_BUILD_JOBS="${CARGO_BUILD_JOBS:-4}"

cef_arch_dir() {
  case "$(uname -m)" in
    arm64|aarch64) echo "cef_macos_aarch64" ;;
    *) echo "cef_macos_x86_64" ;;
  esac
}

# Prints the Chromium Embedded Framework.framework folder from the cache.
find_cef_framework() {
  local d
  d="$(find "$CEF_PATH" -maxdepth 3 -type d -path "*/$(cef_arch_dir)/Chromium Embedded Framework.framework" | head -n 1)"
  [ -n "$d" ] || return 1
  echo "$d"
}

# Version of [workspace.package] in Cargo.toml.
workspace_version() {
  local v
  v="$(awk '/^\[/{s=($0=="[workspace.package]")} s && /^version *=/{gsub(/[" ]/,"",$0); sub(/^version=/,"",$0); print; exit}' "$ROOT/Cargo.toml")"
  echo "${v:-0.5.0}"
}

# Prints the CEF folder of the current Linux machine from the cache ($CEF_PATH/<cef version>/cef_linux_<arch>).
# On Linux (and Windows) cef-dll-sys flattens Release/ and Resources/ into that folder: libcef.so, *.pak,
# icudtl.dat, locales/, v8_context_snapshot.bin... sit side by side, next to the SDK folders (include, cmake...).
find_cef_linux() {
  local arch
  case "$(uname -m)" in
    arm64|aarch64) arch="aarch64" ;;
    armv7l|arm) arch="arm" ;;
    *) arch="x86_64" ;;
  esac
  local d
  d="$(find "$CEF_PATH" -maxdepth 3 -type d -name "cef_linux_$arch" | head -n 1)"
  [ -n "$d" ] && [ -f "$d/libcef.so" ] || return 1
  echo "$d"
}

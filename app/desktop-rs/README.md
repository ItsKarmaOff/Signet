# Signet in Rust (GPUI + CEF)

This folder is a full rewrite of the desktop app (`app/desktop`, Electron) in Rust. The interface is drawn with
[GPUI](https://www.gpui.rs) and [`gpui-component`](https://github.com/longbridge/gpui-component), the web pages are rendered
by Chromium through [CEF](https://github.com/tauri-apps/cef-rs) 156. The behavior is the one of [docs/SPEC.md](../../docs/SPEC.md).
The Electron app is not touched and can be installed next to this one (different bundle id and data folder).

- Plan, crate map and rules for contributors: [PORTING.md](PORTING.md).
- What to check by hand once everything is assembled: [TESTING.md](TESTING.md).
- Crate READMEs: [signet-web](crates/signet-web/README.md) (CEF layer, bundle layout), [signet-link](crates/signet-link/README.md)
  (phone link, passkeys, Bluetooth and local network requirements), and the `README.md` of every other crate in `crates/`.

## Prerequisites

Common: Rust (stable, edition 2024, so 1.85 or newer; `rustup update stable`), `cmake` 3.21+, `ninja`, a C++ toolchain, `git`,
and a network connection for the first build (it downloads CEF, about 500 MB, once).

| Platform | What to install |
| -------- | --------------- |
| macOS 11+ (Apple Silicon or Intel) | Xcode (full app, not only the command line tools), then the Metal toolchain `xcodebuild -downloadComponent MetalToolchain` (GPUI compiles its shaders with it), `brew install cmake ninja` |
| Linux (x86_64 or aarch64) | Debian/Ubuntu: `sudo apt install build-essential cmake ninja-build pkg-config libxkbcommon-dev libxkbcommon-x11-dev libxcb1-dev libx11-dev libwayland-dev libvulkan-dev libdbus-1-dev libgtk-3-dev libnss3 libatk1.0-0 libatk-bridge2.0-0 libcups2 libdrm2 libgbm1 libasound2 libfontconfig1-dev libfreetype-dev libssl-dev`. Runtime: a Vulkan driver (`mesa-vulkan-drivers`), BlueZ (`bluetoothd`) for passkeys with a phone, an X11 or XWayland session (see below). Fedora equivalents: `libxkbcommon-devel vulkan-loader-devel dbus-devel gtk3-devel nss atk at-spi2-atk cups-libs libdrm mesa-libgbm alsa-lib`. |
| Windows 10+ (x64 or arm64) | Visual Studio 2022 Build Tools with "Desktop development with C++" (MSVC and a Windows 11 SDK), `winget install Kitware.CMake Ninja-build.Ninja`, optionally `mt.exe` (part of the SDK, used to embed the manifest). Build from a "x64 Native Tools" prompt or a PowerShell where `cl.exe` is on the PATH. |

The package lists for Linux are a starting point written without running them here: install what the linker or CEF reports as missing.

## CEF distribution

`cef-dll-sys` (a dependency of the `cef` crate) downloads the CEF build that matches the crate version during `cargo build`, and
extracts it to `$CEF_PATH/<cef version>/cef_<os>_<arch>`, for example `.cache/cef/156.0.15/cef_macos_aarch64`.

- `CEF_PATH` defaults to `app/desktop-rs/.cache/cef` in the scripts (git-ignored). Set it to share one copy between checkouts.
  When you call `cargo` yourself, export it too (`export CEF_PATH=$PWD/.cache/cef`), or a fresh download lands in each `target/` folder.
- An existing copy is reused as is (checked against its `archive.json`). Delete the folder to force a new download.
- macOS keeps the `Chromium Embedded Framework.framework`. On Linux and Windows the `Release/` and `Resources/` folders of the
  distribution are merged in one folder (`libcef.so` or `libcef.dll`, `*.pak`, `icudtl.dat`, `locales/`, `v8_context_snapshot.bin`,
  Vulkan and ANGLE libraries), and `cef-dll-sys` also copies those files next to the binaries in `target/<profile>/`.
- `FLATPAK=1` makes the build use the CEF installed in `/usr/lib`; `NIX_CEF_BINARY` is supported too (see the `cef-dll-sys` build script).

## Build, run, bundle

Always run cargo from this folder (never from inside a crate).

```sh
cd app/desktop-rs
export CEF_PATH=$PWD/.cache/cef CARGO_BUILD_JOBS=4

# Unit tests of one crate. The crates without GPUI or CEF use their own target folder (PORTING.md, section 4):
CARGO_TARGET_DIR=target-core cargo test -p signet-core
cargo test -p signet-ui
```

The app only runs from a bundle, because CEF needs its framework and helper processes next to the executable.

| Platform | Bundle | Run | Result |
| -------- | ------ | --- | ------ |
| macOS | `scripts/bundle-macos.sh [--release] [--sign "<identity>"]` | `scripts/run-macos.sh [--release] [-- app args]` | `dist/Signet.app` |
| Linux | `scripts/bundle-linux.sh [--release] [--tar]` | `dist/signet-linux/signet` | `dist/signet-linux/` |
| Windows | `powershell -File scripts\bundle-windows.ps1 [-Release] [-Zip]` | `dist\signet-windows\signet.exe` | `dist\signet-windows\` |

Debug is the default everywhere. `--release` builds with optimizations and strips symbols (macOS and Linux) to keep the bundle small.
`dist/` is git-ignored.

### macOS bundle

`Signet.app` (id `dev.signet.app.gpui`, version taken from `[workspace.package]` in `Cargo.toml`) contains the app, the CEF framework and
five helper apps (`Signet Helper`, `(GPU)`, `(Renderer)`, `(Plugin)`, `(Alerts)`). The `Info.plist` declares:

- usage texts: camera, microphone, location, Bluetooth (passkeys with a phone), local network plus `NSBonjourServices` `_signet._tcp` (phone link);
- default browser registration: `CFBundleURLTypes` for `http`, `https` and `signet`, `CFBundleDocumentTypes` for HTML and XHTML files;
- `LSMinimumSystemVersion` 11.0, `NSHighResolutionCapable`, `NSSupportsAutomaticGraphicsSwitching`.

Run from a terminal during development, macOS attributes Bluetooth and local network access to the terminal app: give it the permission, or
launch the bundle with `open dist/Signet.app`.

### Signing and notarizing (macOS)

Without `--sign` the bundle is ad hoc signed: fine on the machine that built it. For distribution:

```sh
scripts/bundle-macos.sh --release --sign "Developer ID Application: Your Name (TEAMID)"
ditto -c -k --keepParent dist/Signet.app dist/Signet.zip
xcrun notarytool submit dist/Signet.zip --keychain-profile "<profile>" --wait   # profile made with: xcrun notarytool store-credentials
xcrun stapler staple dist/Signet.app
```

`--sign` signs inside out (framework, helper apps, app) with the hardened runtime and a secure timestamp, and gives the executables
`scripts/macos/entitlements.plist`: JIT and unsigned executable memory (V8), network client and server, Bluetooth, camera, microphone, location.
Then check with `codesign --verify --strict --verbose=2 dist/Signet.app` and `spctl --assess --type execute --verbose dist/Signet.app`.
The CEF sandbox is off for now (`no_sandbox`, see the signet-web README), which is allowed for notarized apps.

### Linux bundle

`dist/signet-linux/` holds `signet` (a launcher), `signet-bin`, `signet-helper`, the CEF runtime files, `signet.desktop` and the icons, with an rpath of
`$ORIGIN` (set with `patchelf` when available, else with a linker argument, which rebuilds once). The launcher runs the app under
XWayland when a Wayland session also has an X server: CEF cannot embed child windows in a Wayland window. The folder can be turned into
a tar.gz (`--tar`), deb/rpm or AppImage: the notes are at the top of `scripts/bundle-linux.sh`. `chrome-sandbox` is shipped but not made SUID,
because the sandbox is off.

### Windows bundle

`dist\signet-windows\` holds `signet.exe`, `signet-helper.exe`, the CEF DLLs and data, `signet.ico` and the application manifest
(per-monitor v2 DPI awareness, Windows 7 to 11 support, long paths, UTF-8), embedded with `mt.exe` when it exists, else written as
`signet.exe.manifest`. Embedding the icon in the exe itself is not done by the script (no `.rc` step in the crates yet).
The first launch of the phone link triggers the Windows Firewall prompt: allow private networks.

## CI

`.github/workflows/rust.yml` ("Desktop (Rust)") runs on pushes to `main`, `dev` and `feat/gpui-desktop`, on pull requests to `main`/`dev` and by hand
(Actions tab, Run workflow), only when `app/desktop-rs/**` or the workflow changes. The Electron workflows are separate.

| Step (each of ubuntu, macos and windows runners) | What it proves |
| ------------------------------------------------ | -------------- |
| `cargo test --workspace` | the whole workspace compiles, links and its unit tests pass |
| `scripts/bundle-{macos.sh,linux.sh,windows.ps1}` (release) | the bundle layout (CEF runtime, helper, manifest, icons) is complete |
| `scripts/selftest.sh --release` | the bundle starts and every `[selftest] PASS|FAIL` line passes; Linux runs under `xvfb-run` with `openbox`, macOS and Windows use the runner desktop |

`cargo fmt --check` is not run: the tree is not rustfmt-clean yet. CEF (`CEF_PATH`), `target/` and the cargo registry are cached per system
(saved even when a step fails, so a first long build is not lost).

Artifacts of a run (Actions tab, open the run, "Artifacts"; kept 14 days): `signet-<OS>` (the bundle), `selftest-<OS>` (`run.log`, the
self-test log, and `screenshot*.png` taken while the app runs). With the CLI: `gh run download <run id> -n selftest-Linux`.

## Where data lives

- Data folder: `<config dir>/Signet GPUI/`
  - macOS `~/Library/Application Support/Signet GPUI/`
  - Linux `~/.config/Signet GPUI/`
  - Windows `%APPDATA%\Signet GPUI\`
- Inside: the JSON files (`state.json`, `history.json`, `bookmarks.json`, `autofill.json`, ...) in the same format as the Electron app, `notes/`, and the CEF
  root cache `cef/` (one `profile-<id>` folder per persistent profile).
- First launch: when this folder is empty and the Electron folder `<config dir>/Signet/` exists, its JSON files and notes are **copied** (the Electron
  folder is never written). Delete `Signet GPUI/` to run the import again.
- The build cache is `app/desktop-rs/.cache/cef` (CEF) and `app/desktop-rs/target*/` (cargo); both are git-ignored.

## Troubleshooting

- `xcrun: error: unable to find utility "metal"`: install the Metal toolchain (command above).
- The CEF download is repeated at every build: `CEF_PATH` is not exported, or points to a folder that fails the `archive.json` check.
- Blank window or "cannot load the CEF framework" on macOS: you ran the bare binary from `target/`. Use the bundle.
- Linux: `libcef.so: cannot open shared object file`: run through `dist/signet-linux/signet` (rpath and `LD_LIBRARY_PATH`).
- Linux, passkeys: `bluetoothd` must run and the adapter be powered on; the scan never turns it on.

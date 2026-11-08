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

## Packaging

`.github/workflows/rust-build.yml` ("Build (Rust)", manual run only for now, no tag trigger, no release) produces the same distributable files as the
Electron release (`.github/workflows/build.yml`), so the release can switch to the Rust app at migration time (the comment at the top of the workflow
lists the steps). Each package script runs the release bundle script first (`--skip-bundle` reuses `dist/`) and writes into `dist/`.

| System | Script | Files (`<v>` = version of `[workspace.package]`, which must equal the root `package.json`; checked by the workflow) |
| ------ | ------ | ------ |
| macOS arm64 / x64 | `scripts/package-macos.sh` | `Signet-<v>-arm64.dmg`, `Signet-<v>-arm64.zip`, `Signet-<v>-x64.dmg`, `Signet-<v>-x64.zip` (ad hoc signed app, DMG with an Applications link, ZIP made with `ditto --keepParent`) |
| Linux x64 | `scripts/package-linux.sh` | `Signet-<v>-x86_64.AppImage`, `Signet-<v>-amd64.deb`, `Signet-<v>-x86_64.rpm` |
| Linux arm64 | same, on an arm64 machine | `Signet-<v>-arm64.AppImage`, `Signet-<v>-arm64.deb`, `Signet-<v>-aarch64.rpm` |
| Windows x64 | `scripts/package-windows.ps1` (NSIS, `scripts/windows/installer.nsi`) | `Signet-<v>-x64-setup.exe` (unsigned) |

CEF is per architecture, so every package is built natively: `macos-latest` (arm64), `macos-15-intel` (x64), `ubuntu-latest`, `ubuntu-24.04-arm`,
`windows-latest`. Artifacts: `Signet-Rust-<system>-<arch>` per job, and `Signet-Rust-metadata` with the update files.

- **Linux deb and rpm**: package `signet`, installed in `/opt/Signet/`, `/usr/bin/signet` link, `/usr/share/applications/signet.desktop`
  (`Exec=/opt/Signet/signet %U`, categories `Network;WebBrowser;`, `MimeType` html, xhtml, `x-scheme-handler/http`, `https` and `signet`, so it can be set
  as the default browser), icons in `/usr/share/icons/hicolor/`. The AppImage is built with `appimagetool` 1.9.0 (downloaded and pinned in the script,
  `AppRun` starts the bundle launcher). No SUID bit anywhere (CEF sandbox is off).
- **Windows installer**: per user (no administrator), `%LOCALAPPDATA%\Programs\Signet`, directory page, Start Menu and desktop shortcuts,
  `Uninstall Signet.exe`, silent mode `/S` and `/D=<dir>`, and the electron-updater switches `--updated /S --force-run` (the app is relaunched at the end).
  The "Apps & features" key is `HKCU\Software\Microsoft\Windows\CurrentVersion\Uninstall\{ea93c96e-a71c-5caa-a6d0-f986425b44b7}`, the one electron-builder
  derives from the appId `dev.signet.app`: installing over an Electron install first runs its uninstaller (user data kept) then replaces it. Default browser:
  `HKCU\Software\Clients\StartMenuInternet\Signet` (Capabilities with `.htm .html .xht .xhtml`, `http`, `https`, `signet`), `RegisteredApplications`,
  program ids `SignetHTML` and `SignetURL`, and the `signet:` scheme.
- **Update metadata**: `scripts/update-metadata.sh <folder> [version]` writes, for the packages found in the folder, electron-builder's files, which
  electron-updater reads from the release: `latest.yml` (Windows), `latest-linux.yml` (x64), `latest-linux-arm64.yml`, `latest-mac.yml` (both Mac
  architectures, ZIPs first, as electron-updater uses the ZIP). Format:

  ```yaml
  version: 0.5.0
  files:
    - url: Signet-0.5.0-x64.zip      # file name, relative to the release download URL
      sha512: <base64 of the SHA-512 digest>
      size: 126828384
    - url: ...
  path: Signet-0.5.0-x64.zip          # the first file
  sha512: <same digest as the first file>
  releaseDate: '2026-11-02T08:11:19.414Z'
  ```

  Order of `files`: macOS x64 zip, arm64 zip, x64 dmg, arm64 dmg; Linux AppImage, deb, rpm; Windows the installer. The workflow's `metadata` job runs it on
  all the artifacts together. `.blockmap` files (differential download) are not produced: electron-updater then downloads the whole file, so
  `blockMapSize` is absent from the AppImage entries. Not done: signing (Mac stays ad hoc, Windows unsigned) and the Windows arm64 installer (the Electron release has none).
- The bundle identifier of the Rust Mac app is `dev.signet.app.gpui` and its data folder `Signet GPUI` (the Electron one is `dev.signet.app`): they must be
  decided at migration time, they are not changed by the packaging.

## CI

`.github/workflows/rust.yml` ("Desktop (Rust)") runs on pushes to `main`, `dev` and `feat/gpui-desktop`, on pull requests to `main`/`dev` and by hand
(Actions tab, Run workflow), only when `app/desktop-rs/**` or the workflow changes. The Electron workflows are separate.

| Step (each of ubuntu, macos and windows runners) | What it proves |
| ------------------------------------------------ | -------------- |
| `cargo test --workspace` | the whole workspace compiles, links and its unit tests pass |
| `scripts/bundle-{macos.sh,linux.sh,windows.ps1}` (release) | the bundle layout (CEF runtime, helper, manifest, icons) is complete |
| `scripts/selftest.sh --release` | the bundle starts and every `[selftest] PASS|FAIL` line passes; Linux runs under `xvfb-run` with `openbox`, macOS and Windows use the runner desktop |

Checks that need a focused window are listed in `ALLOW_FAIL` of the workflow (macOS: key window checks of the space form,
sidebar toggle and welcome window; Linux and Windows: `win.welcome`): `scripts/selftest.sh` prints them as "known environment limit"
and does not fail on them, any other FAIL fails the job. On Linux the CEF windows live in a small wrapper window created with the
screen's default depth (GPUI windows use a 32-bit visual, in which Chromium cannot create its child window).

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

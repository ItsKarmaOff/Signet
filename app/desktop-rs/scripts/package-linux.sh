#!/usr/bin/env bash
# Makes the Linux distributable files from a release bundle (dist/signet-linux): AppImage, deb and rpm, named like the
# Electron ones: Signet-<version>-<x86_64|arm64>.AppImage, Signet-<version>-<amd64|arm64>.deb,
# Signet-<version>-<x86_64|aarch64>.rpm. Built for the machine's architecture (CEF is per architecture).
#   scripts/package-linux.sh [--skip-bundle] [--only appimage|deb|rpm]
# Needs: dpkg-deb (deb), rpmbuild (rpm: apt install rpm), curl (appimagetool is downloaded, pinned, into .cache/).
# Install layout of deb and rpm (same as electron-builder's): /opt/Signet/ (the bundle), /usr/bin/signet (link),
# /usr/share/applications/signet.desktop, /usr/share/icons/hicolor/<size>/apps/signet.png.
source "$(dirname "${BASH_SOURCE[0]}")/common.sh"

[ "$(uname -s)" = Linux ] || { echo "package-linux.sh only runs on Linux" >&2; exit 1; }

SKIP=0
ONLY="appimage deb rpm"
while [ $# -gt 0 ]; do
  case "$1" in
    --skip-bundle) SKIP=1 ;;
    --only) ONLY="$2"; shift ;;
    *) echo "unknown flag: $1" >&2; exit 2 ;;
  esac
  shift
done

APPIMAGETOOL_VERSION=1.9.0
VERSION="$(workspace_version)"
MACHINE="$(uname -m)"
case "$MACHINE" in
  aarch64|arm64) APPIMAGE_ARCH=arm64; DEB_ARCH=arm64; RPM_ARCH=aarch64; TOOL_ARCH=aarch64 ;;
  *) APPIMAGE_ARCH=x86_64; DEB_ARCH=amd64; RPM_ARCH=x86_64; TOOL_ARCH=x86_64 ;;
esac
DIST="$ROOT/dist"
BUNDLE="$DIST/signet-linux"
MAINTAINER="Christophe Vandevoir <115950432+ItsKarmaOff@users.noreply.github.com>"
HOMEPAGE="https://github.com/ItsKarmaOff/Signet"
SUMMARY="A sidebar-first web browser: spaces, command bar, split view, Little Signet and Peek."

[ "$SKIP" = 1 ] || "$ROOT/scripts/bundle-linux.sh" --release
[ -f "$BUNDLE/signet-bin" ] || { echo "$BUNDLE not found (run without --skip-bundle)" >&2; exit 1; }

WORK="$(mktemp -d "${TMPDIR:-/tmp}/signet-pkg.XXXXXX")"
trap 'rm -rf "$WORK"' EXIT

want() { case " $ONLY " in *" $1 "*) return 0 ;; *) return 1 ;; esac; }

# Desktop entry for the installed packages (the bundle's own one has Exec=signet and is used by the AppImage).
desktop_file() { # desktop_file <exec> <output>
  cat > "$2" <<DESKTOP
[Desktop Entry]
Type=Application
Name=Signet
GenericName=Web Browser
Comment=Browse the web with spaces, folders and split views
Exec=$1 %U
Icon=signet
Terminal=false
Categories=Network;WebBrowser;
MimeType=text/html;text/xml;application/xhtml+xml;x-scheme-handler/http;x-scheme-handler/https;x-scheme-handler/signet;
StartupNotify=true
StartupWMClass=signet
DESKTOP
}

# Files shared by deb and rpm, laid out under $1 (a package root).
stage_root() {
  local r="$1"
  mkdir -p "$r/opt/Signet" "$r/usr/bin" "$r/usr/share/applications"
  cp -a "$BUNDLE/." "$r/opt/Signet/"
  # The desktop file and icons are installed system-wide, not left in /opt.
  rm -f "$r/opt/Signet/signet.desktop"
  rm -rf "$r/opt/Signet/icons"
  ln -s /opt/Signet/signet "$r/usr/bin/signet"
  desktop_file /opt/Signet/signet "$r/usr/share/applications/signet.desktop"
  if [ -d "$BUNDLE/icons" ]; then
    mkdir -p "$r/usr/share/icons"
    cp -a "$BUNDLE/icons/hicolor" "$r/usr/share/icons/"
  fi
  # No SUID bit anywhere: the CEF sandbox is off.
  chmod -R go-w "$r/opt/Signet"
}

AFTER_INSTALL='if command -v update-desktop-database >/dev/null 2>&1; then update-desktop-database -q /usr/share/applications || true; fi
if command -v gtk-update-icon-cache >/dev/null 2>&1; then gtk-update-icon-cache -q -t -f /usr/share/icons/hicolor || true; fi'

# ---------------------------------------------------------------- deb
if want deb; then
  command -v dpkg-deb >/dev/null 2>&1 || { echo "dpkg-deb not found" >&2; exit 1; }
  R="$WORK/deb"
  stage_root "$R"
  mkdir -p "$R/DEBIAN"
  SIZE="$(du -sk "$R" | cut -f1)"
  cat > "$R/DEBIAN/control" <<CONTROL
Package: signet
Version: $VERSION
Architecture: $DEB_ARCH
Maintainer: $MAINTAINER
Installed-Size: $SIZE
Depends: libgtk-3-0 | libgtk-3-0t64, libnss3, libxkbcommon0, libxkbcommon-x11-0, libxcb1, libx11-6, libdbus-1-3, libvulkan1, libasound2 | libasound2t64, libgbm1, libdrm2, libcups2 | libcups2t64, xdg-utils
Recommends: bluez, mesa-vulkan-drivers
Section: web
Priority: optional
Homepage: $HOMEPAGE
Description: $SUMMARY
CONTROL
  printf '#!/bin/sh\nset -e\n%s\nexit 0\n' "$AFTER_INSTALL" > "$R/DEBIAN/postinst"
  printf '#!/bin/sh\nset -e\n%s\nexit 0\n' "$AFTER_INSTALL" > "$R/DEBIAN/postrm"
  chmod 755 "$R/DEBIAN/postinst" "$R/DEBIAN/postrm"
  OUT="$DIST/Signet-$VERSION-$DEB_ARCH.deb"
  rm -f "$OUT"
  dpkg-deb --root-owner-group -Zxz --build "$R" "$OUT" >/dev/null
  echo "Package: $OUT"
fi

# ---------------------------------------------------------------- rpm
if want rpm; then
  command -v rpmbuild >/dev/null 2>&1 || { echo "rpmbuild not found (apt install rpm)" >&2; exit 1; }
  R="$WORK/rpm-root"
  stage_root "$R"
  TOP="$WORK/rpmbuild"
  mkdir -p "$TOP"/{BUILD,RPMS,SPECS,SRPMS}
  # '-' is not allowed in an rpm version: 1.0.0-beta.1 becomes 1.0.0~beta.1 (sorts before 1.0.0).
  RPM_VERSION="${VERSION//-/\~}"
  cat > "$TOP/SPECS/signet.spec" <<SPEC
Name: signet
Version: $RPM_VERSION
Release: 1
Summary: $SUMMARY
License: MIT
URL: $HOMEPAGE
Packager: $MAINTAINER
# The CEF libraries are bundled: no automatic dependency scan, the system ones are listed by hand.
AutoReqProv: no
Requires: gtk3, nss, libxkbcommon, libxkbcommon-x11, libxcb, libX11, dbus-libs, alsa-lib, mesa-libgbm, libdrm, cups-libs, xdg-utils
Recommends: bluez
%global debug_package %{nil}
%global __os_install_post %{nil}
%global _build_id_links none
%global _binary_payload w19.xzdio

%description
$SUMMARY

%install
cp -a $R/. %{buildroot}/

%post
$AFTER_INSTALL

%postun
$AFTER_INSTALL

%files
%defattr(-,root,root,-)
/opt/Signet
/usr/bin/signet
/usr/share/applications/signet.desktop
/usr/share/icons/hicolor
SPEC
  rpmbuild -bb --target "$RPM_ARCH" --define "_topdir $TOP" "$TOP/SPECS/signet.spec" >"$WORK/rpmbuild.log" 2>&1 \
    || { cat "$WORK/rpmbuild.log" >&2; exit 1; }
  OUT="$DIST/Signet-$VERSION-$RPM_ARCH.rpm"
  rm -f "$OUT"
  cp "$TOP"/RPMS/*/signet-*.rpm "$OUT"
  echo "Package: $OUT"
fi

# ---------------------------------------------------------------- AppImage
if want appimage; then
  TOOL="$ROOT/.cache/appimagetool-$APPIMAGETOOL_VERSION-$TOOL_ARCH.AppImage"
  if [ ! -x "$TOOL" ]; then
    mkdir -p "$ROOT/.cache"
    curl -fsSL -o "$TOOL" "https://github.com/AppImage/appimagetool/releases/download/$APPIMAGETOOL_VERSION/appimagetool-$TOOL_ARCH.AppImage"
    chmod +x "$TOOL"
  fi
  A="$WORK/Signet.AppDir"
  mkdir -p "$A/usr/lib/signet"
  cp -a "$BUNDLE/." "$A/usr/lib/signet/"
  rm -f "$A/usr/lib/signet/signet.desktop"
  rm -rf "$A/usr/lib/signet/icons"
  cat > "$A/AppRun" <<'APPRUN'
#!/bin/sh
here="$(dirname "$(readlink -f "$0")")"
exec "$here/usr/lib/signet/signet" "$@"
APPRUN
  chmod 755 "$A/AppRun"
  desktop_file signet "$A/signet.desktop"
  cp "$BUNDLE/signet.png" "$A/signet.png"
  ln -s signet.png "$A/.DirIcon"
  if [ -d "$BUNDLE/icons" ]; then
    mkdir -p "$A/usr/share/icons"
    cp -a "$BUNDLE/icons/hicolor" "$A/usr/share/icons/"
  fi
  OUT="$DIST/Signet-$VERSION-$APPIMAGE_ARCH.AppImage"
  rm -f "$OUT"
  # --appimage-extract-and-run: the runner has no FUSE. --no-appstream: no appstream metadata shipped.
  ARCH="$TOOL_ARCH" "$TOOL" --appimage-extract-and-run --no-appstream "$A" "$OUT" >"$WORK/appimagetool.log" 2>&1 \
    || { cat "$WORK/appimagetool.log" >&2; exit 1; }
  chmod 755 "$OUT"
  echo "Package: $OUT"
fi

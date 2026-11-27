#!/bin/sh
# SPDX-FileCopyrightText: 2026 Axolotl Team
# SPDX-License-Identifier: GPL-3.0-only
#
# Build the Debian package from an already built binary.
#
# Usage: package-deb.sh <axlauncher-binary> <output-directory>

set -eu

binary=$1
outdir=$2
root=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
version=$(grep -m1 '^version = ' "$root/Cargo.toml" | cut -d'"' -f2)

stage=$(mktemp -d)
trap 'rm -rf "$stage"' EXIT

mkdir -p "$stage/DEBIAN" \
  "$stage/usr/bin" \
  "$stage/usr/share/applications" \
  "$stage/usr/share/icons/hicolor/512x512/apps" \
  "$stage/usr/share/doc/axlauncher"

install -m755 "$binary" "$stage/usr/bin/axlauncher"
install -m644 "$root/packaging/linux/com.axolotl.axlauncher.desktop" \
  "$stage/usr/share/applications/com.axolotl.axlauncher.desktop"
install -m644 "$root/packaging/icons/axolotl-launcher.png" \
  "$stage/usr/share/icons/hicolor/512x512/apps/axolotl-launcher.png"
install -m644 "$root/LICENSES/GPL-3.0-only.txt" \
  "$stage/usr/share/doc/axlauncher/copyright"

cat >"$stage/DEBIAN/control" <<CONTROL
Package: axlauncher
Version: $version
Section: games
Priority: optional
Architecture: amd64
Installed-Size: $(du -sk --exclude=DEBIAN "$stage" | cut -f1)
Depends: libc6 (>= 2.35), libgcc-s1, libxcb1, libxkbcommon0, libxkbcommon-x11-0, libfontconfig1, libfreetype6
Maintainer: Axolotl Team <xastronot233@gmail.com>
Homepage: https://github.com/Axolotl-Launcher/Axolotl-2.0
Description: Minecraft launcher
 Axolotl Launcher installs, launches and manages Minecraft instances. It reads
 the upstream manifests, bundles no Mojang code, and keeps its own data under
 AXLauncher in the system data directory.
CONTROL

mkdir -p "$outdir"
dpkg-deb --root-owner-group --build "$stage" "$outdir/axlauncher_${version}_amd64.deb"

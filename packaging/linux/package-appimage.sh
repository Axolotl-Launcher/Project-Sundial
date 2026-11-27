#!/bin/sh
# SPDX-FileCopyrightText: 2026 Axolotl Team
# SPDX-License-Identifier: GPL-3.0-only
#
# Build the AppImage from the portable tree.
#
# Usage: package-appimage.sh <portable-tree> <output-directory>

set -eu

tree=$1
outdir=$2
root=$(CDPATH='' cd -- "$(dirname -- "$0")/../.." && pwd)
version=$(grep -m1 '^version = ' "$root/Cargo.toml" | cut -d'"' -f2)

# Upstream publishes the runtime under a moving tag, so the archive is checked
# against this hash instead of being trusted.
runtime_url=https://github.com/AppImage/type2-runtime/releases/download/continuous/runtime-x86_64
runtime_sha256=617aa77314d57c8b8e4fae9c3f0b35af7b30d5529cb69e98f64f92510ed98a46

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

curl -fsSL -o "$work/runtime" "$runtime_url"
echo "$runtime_sha256  $work/runtime" | sha256sum -c -

mkdir -p "$work/AppDir"
cp -a "$tree/." "$work/AppDir/"
# The store copy is read-only; the bundle root still has to take AppRun.
chmod -R u+w "$work/AppDir"

install -m755 "$root/packaging/linux/appimage-apprun.sh" "$work/AppDir/AppRun"
install -m644 "$root/packaging/linux/com.axolotl.axlauncher.desktop" \
  "$work/AppDir/axlauncher.desktop"
install -m644 "$root/packaging/icons/axolotl-launcher.png" "$work/AppDir/axolotl-launcher.png"
ln -sf axolotl-launcher.png "$work/AppDir/.DirIcon"

mksquashfs "$work/AppDir" "$work/app.squashfs" \
  -root-owned -noappend -quiet -comp zstd

mkdir -p "$outdir"
image="$outdir/axlauncher-${version}-x86_64.AppImage"
cat "$work/runtime" "$work/app.squashfs" >"$image"
chmod +x "$image"

# The runtime needs the distribution's loader and runs on the runner, so the
# checks read the layout: an ELF in front, a squashfs exactly behind it.
runtime_size=$(stat -c%s "$work/runtime")
test "$(head -c4 "$image" | od -An -tx1 | tr -d ' \n')" = 7f454c46
test "$(dd if="$image" bs=1 skip="$runtime_size" count=4 2>/dev/null | od -An -tx1 | tr -d ' \n')" = 68737173
unsquashfs -o "$runtime_size" -d "$work/root" "$image" >/dev/null
test -x "$work/root/AppRun"
test -x "$work/root/bin/axlauncher"
test -d "$work/root/share/xkb"

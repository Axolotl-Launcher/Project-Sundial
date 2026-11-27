#!/bin/sh
# SPDX-FileCopyrightText: 2026 Axolotl Team
# SPDX-License-Identifier: GPL-3.0-only
#
# Build the RPM package from an already built binary.
#
# Usage: package-rpm.sh <axlauncher-binary> <output-directory>

set -eu

binary=$1
outdir=$2
root=$(CDPATH='' cd -- "$(dirname -- "$0")/../.." && pwd)
version=$(grep -m1 '^version = ' "$root/Cargo.toml" | cut -d'"' -f2)

topdir=$(mktemp -d)
trap 'rm -rf "$topdir"' EXIT
for directory in BUILD BUILDROOT RPMS SOURCES SPECS SRPMS db tmp; do
  mkdir -p "$topdir/$directory"
done

install -m755 "$binary" "$topdir/SOURCES/axlauncher"
install -m644 "$root/packaging/linux/com.axolotl.axlauncher.desktop" \
  "$topdir/SOURCES/com.axolotl.axlauncher.desktop"
install -m644 "$root/packaging/icons/axolotl-launcher.png" \
  "$topdir/SOURCES/axolotl-launcher.png"
install -m644 "$root/LICENSES/GPL-3.0-only.txt" "$topdir/SOURCES/copyright"

cat >"$topdir/SPECS/axlauncher.spec" <<SPEC
Name:           axlauncher
Version:        $version
Release:        1
Summary:        Minecraft launcher
License:        GPL-3.0-only
URL:            https://github.com/Axolotl-Launcher/Axolotl-2.0
BuildArch:      x86_64
# The binary links the rest; these two are dlopened and invisible to the
# dependency generator.
Requires:       fontconfig
Requires:       freetype

%global debug_package %{nil}
%global __strip /bin/true
%global _build_id_links none

%description
Axolotl Launcher installs, launches and manages Minecraft instances. It reads
the upstream manifests, bundles no Mojang code, and keeps its own data under
AXLauncher in the system data directory.

%install
install -Dm755 %{_sourcedir}/axlauncher %{buildroot}%{_bindir}/axlauncher
install -Dm644 %{_sourcedir}/com.axolotl.axlauncher.desktop \\
  %{buildroot}%{_datadir}/applications/com.axolotl.axlauncher.desktop
install -Dm644 %{_sourcedir}/axolotl-launcher.png \\
  %{buildroot}%{_datadir}/icons/hicolor/512x512/apps/axolotl-launcher.png
install -Dm644 %{_sourcedir}/copyright %{buildroot}%{_docdir}/axlauncher/copyright

%files
%{_bindir}/axlauncher
%{_datadir}/applications/com.axolotl.axlauncher.desktop
%{_datadir}/icons/hicolor/512x512/apps/axolotl-launcher.png
%{_docdir}/axlauncher/copyright
SPEC

export SOURCE_DATE_EPOCH=0
rpmbuild --define "_topdir $topdir" \
  --define "_prefix /usr" \
  --define "_docdir /usr/share/doc" \
  --define "_dbpath $topdir/db" \
  --define "_tmppath $topdir/tmp" \
  -bb "$topdir/SPECS/axlauncher.spec" >/dev/null

mkdir -p "$outdir"
cp "$topdir/RPMS/x86_64/axlauncher-${version}-1.x86_64.rpm" "$outdir/"

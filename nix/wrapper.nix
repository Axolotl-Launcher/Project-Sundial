# SPDX-FileCopyrightText: 2026 Axolotl Team
# SPDX-License-Identifier: GPL-3.0-only

# The user-facing package: a symlink to the binary plus the desktop entry and
# the icon the entry names.

{
  newest,
  pkgs,
  version,
  meta,
}:
pkgs.runCommand "axlauncher-${version}" { inherit meta; } ''
  mkdir -p "$out/bin"
  ln -s ${newest}/bin/axlauncher "$out/bin/axlauncher"
  install -Dm644 ${../packaging/linux/com.axolotl.axlauncher.desktop} \
    "$out/share/applications/com.axolotl.axlauncher.desktop"
  install -Dm644 ${../packaging/icons/axolotl-launcher.png} \
    "$out/share/icons/hicolor/512x512/apps/axolotl-launcher.png"
''

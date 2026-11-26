<!-- SPDX-FileCopyrightText: 2026 Axolotl Team -->
<!-- SPDX-License-Identifier: GPL-3.0-only -->

# Linux desktop integration

Install the `axlauncher` binary on `PATH`. Install `com.axolotl.axlauncher.desktop`
under `share/applications/` and `../icons/axolotl-launcher.png` under
`share/icons/hicolor/512x512/apps/` in the same installation prefix, such as
`/usr` or `~/.local`. The desktop entry filename matches the Wayland app ID so
the compositor can find the icon. The X11 window also carries an embedded icon.

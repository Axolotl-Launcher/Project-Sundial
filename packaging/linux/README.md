<!-- SPDX-FileCopyrightText: 2026 Axolotl Team -->
<!-- SPDX-License-Identifier: GPL-3.0-only -->

# Linux desktop integration

Install the `axlauncher` binary on `PATH`. Install `com.axolotl.axlauncher.desktop`
under `share/applications/` and `../icons/axolotl-launcher.png` under
`share/icons/hicolor/512x512/apps/` in the same installation prefix, such as
`/usr` or `~/.local`. The desktop entry filename matches the Wayland app ID so
the compositor can find the icon. The X11 window also carries an embedded icon.

## Binaries the flake produces

nix builds the same source twice, and the difference is the glibc the link sees.

| Attribute | Interpreter | RUNPATH | For |
|---|---|---|---|
| `__axlbin.newest` | nix store | store paths, plus `vulkan-loader`, `wayland` and `/run/opengl-driver/lib` | nix users, through the `axlauncher` wrapper |
| `__axlbin.legacy` | `/lib64/ld-linux-x86-64.so.2` | `$ORIGIN/../lib` | deb, rpm |

`__axlbin.legacy` carries no store path, and `checks.legacy-elf` fails the build
if one appears or if the binary needs glibc newer than 2.35 (Ubuntu 22.04).
Both variants link only `libc`, `libm`, `libgcc_s`, `libxkbcommon` and `libxcb`:
fontconfig, FreeType, Vulkan, wayland and EGL are dlopened at runtime, so their
versions come from the host.

`packages.axlauncher-portable` bundles `__axlbin.newest` with the loader, the
libraries it links and the keymap data. It is the AppImage payload and an
internal build product: the release publishes the image, not the tree.
`checks.portable` proves the tree is self-contained, since the packaging job
only checks the layout of what it receives.

The portable build checks that its wrapper can load the verified embedded Java
discovery, installation and Modrinth images, then shut each worker down. These
probe images are not shipped.

## Layout

`$ORIGIN/../lib` fixes the layout a native package must produce:

```
axlauncher/
  bin/axlauncher
  lib/*.so*        # optional
```

`lib/` may be omitted. The loader then falls through to the system libraries, so
one `axlauncher-legacy` serves both a self-contained archive and a deb that
depends on the distribution's libraries.

## Packages

Packaging runs outside nix, on a normal runner with the distribution tools, and
takes the artifacts nix produced:

| Script | Input | Output |
|---|---|---|
| `package-deb.ts` | the legacy binary | `axlauncher_<version>_amd64.deb` |
| `package-rpm.ts` | the legacy binary | `axlauncher-<version>-1.x86_64.rpm` |
| `package-appimage.ts` | the portable tree | `axlauncher-<version>-x86_64.AppImage` |

The deb and the rpm declare the distribution's libraries, since the legacy
binary links or dlopens them. The AppImage carries its own glibc, libraries and
keymap data, and the script verifies the type-2 runtime it downloads against a
recorded hash.

## Build the artifacts

```bash
nix build .#axlauncher-legacy     # native ELF
nix build .#axlauncher-portable   # self-contained tree

node packaging/linux/package-deb.ts result/bin/axlauncher dist
node packaging/linux/package-rpm.ts result/bin/axlauncher dist
node packaging/linux/package-appimage.ts <portable-tree> dist
```

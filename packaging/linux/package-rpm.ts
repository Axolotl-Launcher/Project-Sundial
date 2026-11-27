// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

// Builds the RPM package from an already built binary.
//
// Usage: node packaging/linux/package-rpm.ts <axlauncher-binary> <output-dir>

import { execFileSync } from "node:child_process";
import { copyFileSync, mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

import { EXECUTABLE, READABLE, argument, copyInto, repoRoot, workspaceVersion } from "./package.ts";

const binary = argument(process.argv[2], "axlauncher binary");
const outDir = argument(process.argv[3], "output directory");
const version = workspaceVersion();
const top = mkdtempSync(join(tmpdir(), "axlauncher-rpm-"));
const name = `axlauncher-${version}-1.x86_64.rpm`;

/// rpm reaches for /var/lib/rpm and /var/tmp by default; both are read-only on a
/// runner that installs nothing.
const DIRECTORIES = ["BUILD", "BUILDROOT", "RPMS", "SOURCES", "SPECS", "SRPMS", "db", "tmp"];

try {
  for (const directory of DIRECTORIES) {
    mkdirSync(join(top, directory), { recursive: true });
  }

  const sources = join(top, "SOURCES");
  copyInto(binary, join(sources, "axlauncher"), EXECUTABLE);
  copyInto(
    join(repoRoot, "packaging/linux/com.axolotl.axlauncher.desktop"),
    join(sources, "com.axolotl.axlauncher.desktop"),
    READABLE,
  );
  copyInto(
    join(repoRoot, "packaging/icons/axolotl-launcher.png"),
    join(sources, "axolotl-launcher.png"),
    READABLE,
  );
  copyInto(join(repoRoot, "LICENSES/GPL-3.0-only.txt"), join(sources, "copyright"), READABLE);

  const spec = join(top, "SPECS/axlauncher.spec");
  writeFileSync(
    spec,
    `Name:           axlauncher
Version:        ${version}
Release:        1
Summary:        Minecraft launcher
License:        GPL-3.0-only
URL:            https://github.com/Axolotl-Launcher/Axolotl-2.0
BuildArch:      x86_64
# Vulkan replaced OpenGL as the game's default renderer, so the loader is a
# runtime dependency rather than an optional backend.
Requires:       vulkan-loader
# Dlopen'd by the toolkit, so the dependency generator cannot see them.
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
`,
  );

  // rpm installs itself under a prefix of its own, so its macros would place the
  // payload there; a package has to use the filesystem hierarchy instead.
  execFileSync(
    "rpmbuild",
    [
      "--define",
      `_topdir ${top}`,
      "--define",
      "_prefix /usr",
      "--define",
      "_docdir /usr/share/doc",
      "--define",
      `_dbpath ${join(top, "db")}`,
      "--define",
      `_tmppath ${join(top, "tmp")}`,
      "-bb",
      spec,
    ],
    { stdio: ["ignore", "ignore", "inherit"], env: { ...process.env, SOURCE_DATE_EPOCH: "0" } },
  );

  mkdirSync(outDir, { recursive: true });
  copyFileSync(join(top, "RPMS/x86_64", name), join(outDir, name));
} finally {
  rmSync(top, { recursive: true, force: true });
}

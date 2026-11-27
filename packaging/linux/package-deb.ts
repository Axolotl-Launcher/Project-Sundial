// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

// Builds the Debian package from an already built binary.
//
// Usage: node packaging/linux/package-deb.ts <axlauncher-binary> <output-dir>

import { execFileSync } from "node:child_process";
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

import { EXECUTABLE, READABLE, argument, copyInto, repoRoot, workspaceVersion } from "./package.ts";

const binary = argument(process.argv[2], "axlauncher binary");
const outDir = argument(process.argv[3], "output directory");
const version = workspaceVersion();
const stage = mkdtempSync(join(tmpdir(), "axlauncher-deb-"));

try {
  copyInto(binary, join(stage, "usr/bin/axlauncher"), EXECUTABLE);
  copyInto(
    join(repoRoot, "packaging/linux/com.axolotl.axlauncher.desktop"),
    join(stage, "usr/share/applications/com.axolotl.axlauncher.desktop"),
    READABLE,
  );
  copyInto(
    join(repoRoot, "packaging/icons/axolotl-launcher.png"),
    join(stage, "usr/share/icons/hicolor/512x512/apps/axolotl-launcher.png"),
    READABLE,
  );
  copyInto(
    join(repoRoot, "LICENSES/GPL-3.0-only.txt"),
    join(stage, "usr/share/doc/axlauncher/copyright"),
    READABLE,
  );

  const usage = execFileSync("du", ["-sk", "--exclude=DEBIAN", stage], { encoding: "utf8" });
  const installedSize = /^(\d+)/.exec(usage)?.[1] ?? "0";

  // Vulkan replaced OpenGL as the game's default renderer, so the loader is a
  // runtime dependency rather than an optional backend.
  mkdirSync(join(stage, "DEBIAN"), { recursive: true });
  writeFileSync(
    join(stage, "DEBIAN/control"),
    `Package: axlauncher
Version: ${version}
Section: games
Priority: optional
Architecture: amd64
Installed-Size: ${installedSize}
Depends: libc6 (>= 2.35), libgcc-s1, libxcb1, libxkbcommon0, libxkbcommon-x11-0, libfontconfig1, libfreetype6, libvulkan1
Maintainer: Axolotl Team <xastronot233@gmail.com>
Homepage: https://github.com/Axolotl-Launcher/Axolotl-2.0
Description: Minecraft launcher
 Axolotl Launcher installs, launches and manages Minecraft instances. It reads
 the upstream manifests, bundles no Mojang code, and keeps its own data under
 AXLauncher in the system data directory.
`,
  );

  mkdirSync(outDir, { recursive: true });
  execFileSync(
    "dpkg-deb",
    ["--root-owner-group", "--build", stage, join(outDir, `axlauncher_${version}_amd64.deb`)],
    { stdio: "inherit" },
  );
} finally {
  rmSync(stage, { recursive: true, force: true });
}

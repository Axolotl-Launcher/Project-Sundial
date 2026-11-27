// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

// Builds the AppImage from the portable tree.
//
// Usage: node packaging/linux/package-appimage.ts <portable-tree> <output-dir>

import { execFileSync } from "node:child_process";
import { createHash } from "node:crypto";
import {
  chmodSync,
  cpSync,
  existsSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  readdirSync,
  rmSync,
  statSync,
  symlinkSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

import {
  EXECUTABLE,
  READABLE,
  argument,
  copyInto,
  repoRoot,
  requireTrue,
  workspaceVersion,
} from "./package.ts";

/// Upstream publishes the runtime under a moving tag, so the bytes are pinned.
const RUNTIME_URL =
  "https://github.com/AppImage/type2-runtime/releases/download/continuous/runtime-x86_64";
const RUNTIME_SHA256 = "617aa77314d57c8b8e4fae9c3f0b35af7b30d5529cb69e98f64f92510ed98a46";

/// Owner-write, so the read-only store copy becomes editable.
const WRITABLE = 0o200;

const ELF_MAGIC = Buffer.from("\x7fELF", "latin1");
const SQUASHFS_MAGIC = Buffer.from("hsqs", "ascii");

const tree = argument(process.argv[2], "portable tree");
const outDir = argument(process.argv[3], "output directory");
const version = workspaceVersion();
const work = mkdtempSync(join(tmpdir(), "axlauncher-appimage-"));

/// The store copy is read-only and the bundle root still has to take AppRun.
function makeWritable(path: string): void {
  chmodSync(path, EXECUTABLE);
  for (const entry of readdirSync(path, { withFileTypes: true })) {
    const child = join(path, entry.name);
    if (entry.isDirectory()) {
      makeWritable(child);
    } else if (entry.isFile()) {
      chmodSync(child, statSync(child).mode | WRITABLE);
    }
  }
}

async function downloadRuntime(path: string): Promise<void> {
  const response = await fetch(RUNTIME_URL);
  requireTrue(response.ok, `runtime download failed: ${response.status.toString()}`);
  const bytes = Buffer.from(await response.arrayBuffer());
  requireTrue(
    createHash("sha256").update(bytes).digest("hex") === RUNTIME_SHA256,
    "runtime does not match the recorded hash",
  );
  writeFileSync(path, bytes);
}

try {
  const runtime = join(work, "runtime");
  await downloadRuntime(runtime);

  const appDir = join(work, "AppDir");
  cpSync(tree, appDir, { recursive: true });
  makeWritable(appDir);

  copyInto(join(repoRoot, "packaging/linux/appimage-apprun.sh"), join(appDir, "AppRun"), EXECUTABLE);
  copyInto(
    join(repoRoot, "packaging/linux/com.axolotl.axlauncher.desktop"),
    join(appDir, "axlauncher.desktop"),
    READABLE,
  );
  copyInto(
    join(repoRoot, "packaging/icons/axolotl-launcher.png"),
    join(appDir, "axolotl-launcher.png"),
    READABLE,
  );
  symlinkSync("axolotl-launcher.png", join(appDir, ".DirIcon"));

  const squashfs = join(work, "app.squashfs");
  execFileSync("mksquashfs", [appDir, squashfs, "-root-owned", "-noappend", "-quiet", "-comp", "zstd"], {
    stdio: ["ignore", "ignore", "inherit"],
  });

  mkdirSync(outDir, { recursive: true });
  const image = join(outDir, `axlauncher-${version}-x86_64.AppImage`);
  writeFileSync(image, Buffer.concat([readFileSync(runtime), readFileSync(squashfs)]));
  chmodSync(image, EXECUTABLE);

  // The runtime expects the distribution's loader and runs on the runner, so the
  // checks read the arrangement instead of the image: an ELF in front, a
  // squashfs exactly behind it, and a payload with its entry points.
  const bytes = readFileSync(image);
  const offset = statSync(runtime).size;
  requireTrue(bytes.subarray(0, ELF_MAGIC.length).equals(ELF_MAGIC), "the image does not start with an ELF");
  requireTrue(
    bytes.subarray(offset, offset + SQUASHFS_MAGIC.length).equals(SQUASHFS_MAGIC),
    "the squashfs does not start where the runtime ends",
  );

  const root = join(work, "root");
  execFileSync("unsquashfs", ["-o", offset.toString(), "-d", root, image], { stdio: ["ignore", "ignore", "inherit"] });
  for (const path of ["AppRun", "bin/axlauncher", "libexec/axlauncher", "share/xkb"]) {
    requireTrue(existsSync(join(root, path)), `the payload is missing ${path}`);
  }
} finally {
  rmSync(work, { recursive: true, force: true });
}

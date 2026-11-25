// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

import { execFileSync, spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import { existsSync, mkdirSync, mkdtempSync, readFileSync, renameSync, rmSync } from "node:fs";
import { join, resolve } from "node:path";

const repository = "https://github.com/deterministic-machines/deslop-please.git";
const revision = "f678d10d6e6860e70900ceae919e2d769b0a11ae"; // v0.1.4
const root = resolve(import.meta.dirname, "..");
const patch = join(import.meta.dirname, "deslop-please-0.1.4.patch");
const cache = join(root, "target", "architecture-tool");
const executable = process.platform === "win32" ? "deslop-please.exe" : "deslop-please";

function install(binary: string): void {
  mkdirSync(cache, { recursive: true });
  const staging = mkdtempSync(join(cache, "install-"));
  const source = join(staging, "source");
  try {
    execFileSync("git", ["init", "--quiet", source], { stdio: "inherit" });
    execFileSync("git", ["fetch", "--quiet", "--depth=1", repository, revision], {
      cwd: source,
      stdio: "inherit",
    });
    execFileSync("git", ["checkout", "--quiet", "--detach", "FETCH_HEAD"], {
      cwd: source,
      stdio: "inherit",
    });
    execFileSync("git", ["apply", "--unidiff-zero", patch], { cwd: source, stdio: "inherit" });
    execFileSync("cargo", ["install", "--locked", "--path", source, "--root", staging], {
      cwd: root,
      stdio: "inherit",
    });
    try {
      renameSync(join(staging, "bin", executable), binary);
    } catch (error) {
      if (!existsSync(binary)) throw error;
    }
  } finally {
    rmSync(staging, { recursive: true, force: true });
  }
}

function main(args: string[]): number {
  const build = createHash("sha256").update(revision).update(readFileSync(patch)).digest("hex");
  const binary = join(cache, `${build}-${executable}`);
  if (!existsSync(binary)) install(binary);

  const result = spawnSync(binary, args, { cwd: root, stdio: "inherit" });
  if (result.error) throw result.error;
  return result.status ?? 1;
}

try {
  const [, , ...args] = process.argv;
  process.exitCode = main(args);
} catch (error) {
  console.error("Architecture check failed", error);
  process.exitCode = 1;
}

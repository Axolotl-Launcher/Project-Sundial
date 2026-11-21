// SPDX-FileCopyrightText: 2026 The Superlogical Authors
// SPDX-License-Identifier: GPL-3.0-only

// Runs a turbo task, scoped to the platform shell of the current OS.
//
// turbo has no notion of the host OS, so `turbo run build` would otherwise
// build every platform shell — compiling the same Rust code once per shell
// for a binary only one of them can use.

import { spawnSync } from "node:child_process";

type PlatformPackages = {
  readonly [platform: string]: readonly string[];
};

const PLATFORM_PACKAGES: PlatformPackages = {
  win32: ["@superlogical/platform-windows"],
  linux: ["@superlogical/platform-linux"],
  darwin: ["@superlogical/platform-macos"],
};

/// `node <script>` puts the interpreter and the script path ahead of the
/// arguments meant for this program.
const NODE_ARGUMENT_OFFSET = 2;

function platformFilters(platform: NodeJS.Platform): string[] {
  const packages = PLATFORM_PACKAGES[platform] ?? [];
  return packages.map((name) => `--filter=${name}`);
}

function main(argv: readonly string[]): number {
  const [task, ...rest] = argv;
  if (task === undefined) {
    console.error("usage: node scripts/turbo.ts <task> [...turbo args]");
    return 1;
  }

  const filters = ["--filter=./apps/*", ...platformFilters(process.platform)];
  const result = spawnSync("turbo", ["run", task, ...filters, ...rest], {
    stdio: "inherit",
    shell: process.platform === "win32",
  });

  return result.status ?? 1;
}

process.exit(main(process.argv.slice(NODE_ARGUMENT_OFFSET)));

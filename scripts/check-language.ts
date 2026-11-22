// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

// Rejects JavaScript source files in this repository.
//
// TypeScript is the only language for scripts here, so a `.js` or `.mjs` file
// is a mistake or a regression. Generated output and dependencies are skipped
// by name; everything else that git tracks is checked.

import { execFileSync } from "node:child_process";

const FORBIDDEN_EXTENSIONS = [".js", ".mjs", ".cjs", ".jsx"] as const;

const IGNORED_PATH_SEGMENTS = [
  "node_modules/",
  "target/",
  "dist/",
  ".turbo/",
] as const;

/// ESLint loads its config before any TypeScript tooling exists, so that one
/// file has to be JavaScript. Nothing else may be.
const ALLOWED_FILES = ["eslint.config.js"] as const;

function trackedFiles(): string[] {
  const output = execFileSync("git", ["ls-files"], { encoding: "utf8" });
  return output.split("\n").filter((line) => line.length > 0);
}

function isForbidden(file: string): boolean {
  if (ALLOWED_FILES.includes(file as (typeof ALLOWED_FILES)[number])) {
    return false;
  }
  const ignored = IGNORED_PATH_SEGMENTS.some((segment) => file.includes(segment));
  if (ignored) {
    return false;
  }
  return FORBIDDEN_EXTENSIONS.some((extension) => file.endsWith(extension));
}

function main(): number {
  const offenders = trackedFiles().filter(isForbidden);

  if (offenders.length === 0) {
    return 0;
  }

  console.error(
    "JavaScript files are not allowed; write TypeScript and let Node strip the types:",
  );
  for (const file of offenders) {
    console.error(`  ${file}`);
  }
  return 1;
}

process.exit(main());

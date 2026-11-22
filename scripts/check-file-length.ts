// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

// Rejects code files longer than 500 lines.
//
// With file arguments (the pre-commit hook passes staged paths) only those are
// checked; without arguments the whole tracked tree is. A file that grows past
// the limit is split by role: production code into modules, tests beside them.
// Physical lines count, so comments and blank lines do too.

import { execFileSync } from "node:child_process";
import { readFileSync } from "node:fs";

/// No code file may exceed this many lines.
export const MAX_LINES = 500;

/// `node script.ts [file...]`: skip the interpreter and the script path.
const FIRST_FILE_ARGUMENT = 2;

/// Extensions that carry code this repository writes by hand.
const CODE_EXTENSIONS = [
  ".rs",
  ".ts",
  ".tsx",
  ".js",
  ".jsx",
  ".mjs",
  ".cjs",
] as const;

const IGNORED_PATH_SEGMENTS = [
  "node_modules/",
  "target/",
  "dist/",
  ".turbo/",
] as const;

export function trackedFiles(): string[] {
  const output = execFileSync("git", ["ls-files"], { encoding: "utf8" });
  return output.split("\n").filter((line) => line.length > 0);
}

export function isCodeFile(file: string): boolean {
  if (IGNORED_PATH_SEGMENTS.some((segment) => file.includes(segment))) {
    return false;
  }
  return CODE_EXTENSIONS.some((extension) => file.endsWith(extension));
}

/// Counts physical lines, with or without a trailing newline.
export function countLines(text: string): number {
  if (text === "") {
    return 0;
  }
  const newlines = text.split("\n").length - 1;
  return text.endsWith("\n") ? newlines : newlines + 1;
}

export interface OversizedFile {
  readonly file: string;
  readonly lines: number;
}

/// The code files in `files` that exceed the limit, longest first.
export function oversizedFiles(files: readonly string[]): OversizedFile[] {
  return files
    .filter(isCodeFile)
    .map((file) => ({ file, lines: countLines(readFileSync(file, "utf8")) }))
    .filter(({ lines }) => lines > MAX_LINES)
    .sort((left, right) => right.lines - left.lines);
}

function main(files: readonly string[]): number {
  const targets = files.length === 0 ? trackedFiles() : files;
  const offenders = oversizedFiles(targets);

  if (offenders.length === 0) {
    return 0;
  }

  console.error(
    `Code files must stay at or below ${String(MAX_LINES)} lines; split by role:`,
  );
  for (const { file, lines } of offenders) {
    console.error(`  ${file}: ${String(lines)} lines`);
  }
  return 1;
}

if (import.meta.main) {
  process.exit(main(process.argv.slice(FIRST_FILE_ARGUMENT)));
}

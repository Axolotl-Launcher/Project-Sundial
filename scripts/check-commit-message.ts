// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

// Rejects commit messages that contain non-ASCII text.
//
// The log is read by people and agents whose first language differs, and a
// mixed-language history is harder to search than a uniform one. A name in a
// sign-off trailer is the documented exception, so trailers are exempt while
// the subject and body are not.
//
// A trailer block is the trailing run of `Key: value` lines after a blank line.
// The subject is never a trailer: `feat: add x` starts the message, so it stays
// in scope.

import { readFileSync } from "node:fs";

const TRAILER_LINE = /^[A-Za-z][A-Za-z-]*: \S/;
// Anything outside printable ASCII is non-English text. Lines are checked
// individually, so the pattern never needs to reason about newlines.
const NON_ASCII = /[^ -~]/;

/// Index of the first line of the trailing trailer block, or the line count
/// when the message has none.
function trailerBlockStart(lines: readonly string[]): number {
  let index = lines.length - 1;

  while (index >= 0 && lines[index] === "") {
    index -= 1;
  }

  const lastTrailer = index;
  while (index >= 0 && TRAILER_LINE.test(lines[index] ?? "")) {
    index -= 1;
  }

  if (lastTrailer === index) {
    return lines.length;
  }

  // A trailer block follows a blank line; without one these are body lines.
  const separator = index >= 0 ? lines[index] : undefined;
  return separator === "" ? index + 1 : lines.length;
}

function main(argv: readonly string[]): number {
  const messagePath = argv[0];
  if (messagePath === undefined) {
    console.error("usage: check-commit-message.ts <path-to-commit-message>");
    return 1;
  }

  const lines = readFileSync(messagePath, "utf8").split("\n");
  const checkable = lines.slice(0, trailerBlockStart(lines));
  const offenders = checkable.filter((line) => NON_ASCII.test(line));

  if (offenders.length === 0) {
    return 0;
  }

  console.error("Commit messages must be written in English (ASCII only).");
  console.error("Trailers such as Signed-off-by may hold a name.");
  for (const line of offenders) {
    console.error(`  ${line}`);
  }
  return 1;
}

// `node <script>` puts the interpreter and the script path ahead of the
// arguments meant for this program.
const NODE_ARGUMENT_OFFSET = 2;

process.exit(main(process.argv.slice(NODE_ARGUMENT_OFFSET)));

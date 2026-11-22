// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

import { readFileSync } from "node:fs";

const MAX_LINES = 500;
const NODE_ARGUMENT_OFFSET = 2;

function main(files: readonly string[]): number {
  let exceeded = false;
  for (const file of files) {
    const source = readFileSync(file, "utf8");
    const newlines = source.match(/\n/g)?.length ?? 0;
    const lines = newlines + Number(source.length > 0 && !source.endsWith("\n"));

    if (lines > MAX_LINES) {
      console.error(
        `${file} has ${String(lines)} lines (limit ${String(MAX_LINES)}). Split this Rust file.`,
      );
      exceeded = true;
    }
  }
  return Number(exceeded);
}

process.exit(main(process.argv.slice(NODE_ARGUMENT_OFFSET)));

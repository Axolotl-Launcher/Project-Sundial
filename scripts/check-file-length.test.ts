// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

import assert from "node:assert/strict";
import { mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";

import {
  MAX_LINES,
  countLines,
  isCodeFile,
  oversizedFiles,
} from "./check-file-length.ts";

/// The repository rule this check exists to enforce.
const REQUIRED_LIMIT = 500;
const TWO_LINES = 2;

void test("counts physical lines with or without a trailing newline", () => {
  assert.equal(countLines(""), 0);
  assert.equal(countLines("one"), 1);
  assert.equal(countLines("one\n"), 1);
  assert.equal(countLines("one\ntwo\n"), TWO_LINES);
});

void test("the limit matches the repository rule", () => {
  assert.equal(MAX_LINES, REQUIRED_LIMIT);
});

void test("recognises source files and skips data and generated paths", () => {
  assert.equal(isCodeFile("apps/core/minecraft/launch/src/flow.rs"), true);
  assert.equal(isCodeFile("scripts/check-file-length.ts"), true);
  assert.equal(isCodeFile("eslint.config.js"), true);
  assert.equal(isCodeFile("Cargo.toml"), false);
  assert.equal(isCodeFile("README.md"), false);
  assert.equal(isCodeFile("node_modules/pkg/index.js"), false);
  assert.equal(isCodeFile("target/debug/build.rs"), false);
});

void test("reports only files above the limit, longest first", () => {
  const directory = mkdtempSync(join(tmpdir(), "file-length-"));
  try {
    const over = join(directory, "over.rs");
    const atLimit = join(directory, "at-limit.rs");
    const data = join(directory, "data.toml");
    writeFileSync(over, "line\n".repeat(REQUIRED_LIMIT + 1));
    writeFileSync(atLimit, "line\n".repeat(REQUIRED_LIMIT));
    writeFileSync(data, "line\n".repeat(REQUIRED_LIMIT + 1));

    assert.deepEqual(oversizedFiles([atLimit, over, data]), [
      { file: over, lines: REQUIRED_LIMIT + 1 },
    ]);
  } finally {
    rmSync(directory, { recursive: true, force: true });
  }
});

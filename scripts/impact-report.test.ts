// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

import assert from "node:assert/strict";
import { test } from "node:test";

import { MAX_LISTED_TESTS, TRAVERSAL_DEPTH } from "./impact-analysis.ts";
import {
  REPORT_MARKER,
  renderComment,
  summaryLine,
  type ImpactReport,
  type TestGroup,
} from "./impact-report.ts";

const SHA_LENGTH = 40;
const NODE_COUNT = 5929;
const INDEXED_FILES = 326;
const SHA_PREFIX_LENGTH = 12;
const TEST_COUNT_PLUS_ONE = MAX_LISTED_TESTS + 1;

const group: TestGroup = {
  module: "state",
  mirrored: true,
  directory: "tests/apps/core/state/",
  tests: ["tests/apps/core/state/src/profile.rs"],
};

function report(overrides: Partial<ImpactReport> = {}): ImpactReport {
  return {
    repository: "Axolotl-Launcher/Project-Sundial",
    pullRequest: 34,
    headSha: "a".repeat(SHA_LENGTH),
    baseSha: "b".repeat(SHA_LENGTH),
    analyzedAt: "2026-11-27T00:00:00.000Z",
    analyzer: "codegraph@1.6.4",
    traversalDepth: TRAVERSAL_DEPTH,
    changedFileCount: 1,
    sourceFiles: 1,
    testFiles: 0,
    otherFiles: 0,
    changedFiles: [{ path: "apps/core/state/src/profile.rs", kind: "source" }],
    changedFileListingLimit: 40,
    affectedTestCount: 1,
    testGroups: [group],
    testListingLimit: MAX_LISTED_TESTS,
    touchedCycleCount: 0,
    cyclesByFile: {},
    cycleListingLimit: 5,
    cycles: [],
    truncatedCycles: false,
    index: {
      fileCount: INDEXED_FILES,
      nodeCount: NODE_COUNT,
      languages: ["rust", "typescript"],
      filesWithParseErrors: 0,
    },
    note: null,
    ...overrides,
  };
}

void test("the comment always starts with its update marker", () => {
  assert.ok(renderComment(report()).startsWith(REPORT_MARKER));
});

void test("the summary counts source, test and affected files", () => {
  assert.equal(
    summaryLine(report({ changedFileCount: 3, sourceFiles: 2, testFiles: 1 })),
    "3 changed files · 2 source · 1 test · 1 affected test file",
  );
});

void test("the summary names an empty change instead of showing zeros", () => {
  assert.match(
    summaryLine(report({ changedFileCount: 0, sourceFiles: 0 })),
    /no changed files/,
  );
});

void test("the comment always says the result is advisory", () => {
  assert.match(renderComment(report()), /Advisory only/);
});

void test("the analysis range is shown as short commits", () => {
  assert.match(
    renderComment(report()),
    new RegExp(`\`${"b".slice(0, SHA_PREFIX_LENGTH)}.*\`\\.\\.\``),
  );
});

void test("a mirrored group is labelled as such", () => {
  assert.match(renderComment(report()), /#### Mirrors \(`tests\/apps\/core\/state\/`\)/);
});

void test("a dependent group explains why it is listed", () => {
  const dependent: TestGroup = {
    module: "install",
    mirrored: false,
    directory: "tests/apps/core/install/",
    tests: ["tests/apps/core/install/src/runtime.rs"],
  };
  const markdown = renderComment(report({ testGroups: [dependent], affectedTestCount: 1 }));
  assert.match(markdown, /#### Dependent tests/);
  assert.match(markdown, /Reached through dependencies/);
});

void test("a long group is capped and its remainder counted", () => {
  const tests = Array.from(
    { length: TEST_COUNT_PLUS_ONE },
    (_value, index) => `tests/apps/core/state/src/file${String(index)}.rs`,
  );
  const markdown = renderComment(
    report({ testGroups: [{ ...group, tests }], affectedTestCount: tests.length }),
  );
  assert.match(markdown, /… 1 more in this group/);
  assert.equal(markdown.match(/^- `tests\/apps\/core\/state\/src\/file/gm)?.length, MAX_LISTED_TESTS);
});

void test("no affected tests is stated rather than left blank", () => {
  const markdown = renderComment(report({ testGroups: [], affectedTestCount: 0 }));
  assert.match(markdown, /CodeGraph found no test file/);
});

void test("a touched cycle lists its files in order", () => {
  const cycles = [["apps/core/state/src/javas.rs", "apps/core/state/src/profile.rs"]];
  const markdown = renderComment(
    report({
      touchedCycleCount: 1,
      cyclesByFile: { "apps/core/state/src/javas.rs": 1 },
      cycles,
    }),
  );
  assert.match(
    markdown,
    /`apps\/core\/state\/src\/javas\.rs` → apps\/core\/state\/src\/profile\.rs/,
  );
});

void test("no touched cycle does not claim the change is cycle-free", () => {
  assert.match(renderComment(report()), /not a proof the change is cycle-free/);
});

void test("the advisory note says a missing index fails instead of publishing", () => {
  assert.match(renderComment(report()), /A missing index fails the job/);
});

void test("paths and notes cannot break out of the Markdown", () => {
  const markdown = renderComment(
    report({
      changedFiles: [
        { path: "apps/core/state/src/a|b.rs", kind: "source" },
        { path: "apps/core/state/src/<script>.rs", kind: "source" },
      ],
      note: "line one\n\nline two",
    }),
  );
  assert.match(markdown, /a\\\|b\.rs/);
  assert.match(markdown, /&lt;script&gt;/);
  assert.doesNotMatch(markdown, /<script>/);
  assert.match(markdown, /^> line one$/m);
  assert.match(markdown, /^> line two$/m);
});

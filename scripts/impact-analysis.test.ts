// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

import assert from "node:assert/strict";
import { test } from "node:test";

import {
  MAX_LISTED_CYCLES,
  TRAVERSAL_DEPTH,
  buildReport,
  changedFileKind,
  crateDirectory,
  groupTests,
  mirroredCrate,
  moduleOf,
  touchedCycles,
  type AnalysisContext,
  type BuildReportInput,
} from "./impact-analysis.ts";
import { affectedInputPaths, decodeChangedPaths } from "./impact-pr.ts";

const SHA_LENGTH = 40;
const NODE_COUNT = 5929;
const INDEXED_FILES = 326;
const CHANGED_LIMIT_PLUS_ONE = 41;

const context: AnalysisContext = {
  repository: "Axolotl-Launcher/Project-Sundial",
  pullRequest: 34,
  headSha: "a".repeat(SHA_LENGTH),
  baseSha: "b".repeat(SHA_LENGTH),
  analyzedAt: "2026-11-27T00:00:00.000Z",
  analyzer: "codegraph@1.6.4",
  traversalDepth: TRAVERSAL_DEPTH,
};

function input(overrides: Partial<BuildReportInput> = {}): BuildReportInput {
  return {
    context,
    changedFiles: ["apps/core/state/src/profile.rs"],
    affectedTests: ["tests/apps/core/state/src/profile.rs"],
    affectedLookupOmitted: 0,
    cycles: [],
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

void test("Git paths preserve Unicode, quotes, boundary whitespace and line breaks", () => {
  const paths = [
    "\uFEFFleading-bom.ts",
    "scripts/café notes.ts",
    'scripts/"quoted".ts',
    " edge.ts ",
    "scripts/two\nlines.ts",
  ];
  assert.deepEqual(decodeChangedPaths(Buffer.from(`${paths.join("\0")}\0`)), paths);
  assert.deepEqual(decodeChangedPaths(Buffer.alloc(0)), []);
});

void test("affected lookup excludes paths CodeGraph would rewrite", () => {
  const supported = ["scripts/café notes.ts", "scripts/with\ta tab.ts"];
  const unsupported = [
    " leading.ts",
    "trailing.ts ",
    "scripts/two\nlines.ts",
    "scripts/return\rkey.ts",
    "scripts/literal\\slash.ts",
  ];
  assert.deepEqual(affectedInputPaths([...supported, ...unsupported]), supported);
  assert.deepEqual(affectedInputPaths(unsupported), []);
});

void test("only code files drive the analysis", () => {
  assert.equal(changedFileKind("apps/core/state/src/profile.rs"), "source");
  assert.equal(changedFileKind("tests/apps/core/state/src/profile.rs"), "test");
  assert.equal(changedFileKind("CONTRIBUTING.md"), "other");
  assert.equal(changedFileKind("Cargo.lock"), "other");
  assert.equal(changedFileKind("scripts/impact-pr.ts"), "source");
});

void test("a crate directory drops the lib grouping", () => {
  assert.equal(crateDirectory("apps/core/state/src/profile.rs"), "apps/core/state");
  assert.equal(crateDirectory("apps/core/lib/accounts/src/lib.rs"), "apps/core/lib/accounts");
  assert.equal(crateDirectory("apps/frontend/src/lib.rs"), "apps/frontend");
  assert.equal(crateDirectory("apps/core/lib/src/downloads.rs"), "apps/core/lib");
  assert.equal(crateDirectory("apps/core/src/desktop/java.rs"), "apps/core");
  assert.equal(crateDirectory("tests/apps/core/src/desktop/java.rs"), "tests/apps/core");
  assert.equal(
    crateDirectory("tests/apps/core/lib/download_engine/src/lib/range_recovery.rs"),
    "tests/apps/core/lib/download_engine",
  );
  assert.equal(crateDirectory("scripts/impact-pr.ts"), undefined);
  assert.equal(crateDirectory("apps"), undefined);
});

void test("module labels follow the triage script", () => {
  assert.equal(moduleOf("apps/core/state/src/profile.rs"), "state");
  assert.equal(moduleOf("tests/apps/core/state/src/profile.rs"), "state");
  assert.equal(moduleOf("apps/core/lib/accounts/src/lib.rs"), "domain");
  assert.equal(moduleOf("tests/apps/core/lib/accounts/src/lib.rs"), "domain");
  assert.equal(moduleOf("apps/core/spi/src/lib.rs"), "domain");
  assert.equal(moduleOf("tests/apps/core/spi/src/lib.rs"), "domain");
  assert.equal(moduleOf("apps/core/content/mod_metadata/src/lib.rs"), "mod-metadata");
  assert.equal(moduleOf("apps/core/content/resources/src/lib.rs"), "resources");
  assert.equal(moduleOf("apps/frontend/src/lib.rs"), "frontend");
  assert.equal(moduleOf("apps/ui/src/lib.rs"), "ui");
  assert.equal(moduleOf("platforms/windows/build.rs"), "platforms");
  assert.equal(moduleOf("scripts/impact-pr.ts"), "repository");
  assert.equal(moduleOf("tests/tests/spi_contract.rs"), "repository");
  assert.equal(moduleOf("tests/tools/mmc-convert/src/patches.rs"), "repository");
});

void test("a mirrored test resolves to its crate", () => {
  assert.equal(
    mirroredCrate("tests/apps/core/lib/download_engine/src/lib/retry_tests.rs"),
    "apps/core/lib/download_engine",
  );
  assert.equal(mirroredCrate("tests/tests/spi_contract.rs"), undefined);
  assert.equal(mirroredCrate("apps/core/state/src/profile.rs"), undefined);
});

void test("a mirrored test is grouped before a dependent one", () => {
  const groups = groupTests(
    ["apps/core/state/src/profile.rs"],
    [
      "tests/apps/core/install/src/runtime.rs",
      "tests/apps/core/state/src/profile/discovery.rs",
      "tests/apps/core/state/src/javas.rs",
    ],
  );
  assert.deepEqual(groups, [
    {
      module: "state",
      mirrored: true,
      directory: "apps/core/state/",
      tests: [
        "tests/apps/core/state/src/javas.rs",
        "tests/apps/core/state/src/profile/discovery.rs",
      ],
    },
    {
      module: "install",
      mirrored: false,
      directory: "apps/core/install",
      tests: ["tests/apps/core/install/src/runtime.rs"],
    },
  ]);
});

void test("one crate stays one group however deep its tests nest", () => {
  const groups = groupTests(
    ["apps/core/lib/download_engine/src/lib.rs"],
    [
      "tests/apps/core/lib/download_engine/src/lib/range_recovery.rs",
      "tests/apps/core/lib/download_engine/src/lib/range_identity.rs",
      "tests/apps/core/lib/download_engine/src/streaming.rs",
    ],
  );
  assert.equal(groups.length, 1);
  assert.deepEqual(groups[0], {
    module: "domain",
    mirrored: true,
    directory: "apps/core/lib/download_engine/",
    tests: [
      "tests/apps/core/lib/download_engine/src/lib/range_identity.rs",
      "tests/apps/core/lib/download_engine/src/lib/range_recovery.rs",
      "tests/apps/core/lib/download_engine/src/streaming.rs",
    ],
  });
});

void test("an umbrella crate does not adopt the tests of nested crates", () => {
  const groups = groupTests(
    ["apps/core/src/desktop/java.rs"],
    ["tests/apps/core/lib/download_engine/src/lib/range_recovery.rs"],
  );
  assert.deepEqual(groups, [
    {
      module: "domain",
      mirrored: false,
      directory: "apps/core/lib/download_engine",
      tests: ["tests/apps/core/lib/download_engine/src/lib/range_recovery.rs"],
    },
  ]);
});

void test("a test shared by two changed crates is listed once", () => {
  const groups = groupTests(
    ["apps/core/state/src/profile.rs", "apps/core/state/src/settings.rs"],
    ["tests/apps/core/state/src/profile.rs"],
  );
  assert.equal(groups.length, 1);
  assert.deepEqual(groups[0]?.tests, ["tests/apps/core/state/src/profile.rs"]);
});

void test("only cycles the change touches and the repository owns are reported", () => {
  const cycles = [
    ["apps/core/state/src/profile.rs", "apps/core/state/src/javas.rs"],
    ["apps/core/legacy/src/a.rs", "apps/core/legacy/src/b.rs"],
    ["apps/core/state/src/profile.rs", "target/debug/generated.rs"],
  ];
  assert.deepEqual(touchedCycles(cycles, ["apps/core/state/src/profile.rs"]), [
    ["apps/core/state/src/profile.rs", "apps/core/state/src/javas.rs"],
  ]);
});

void test("counts cover the full change while the listing is capped", () => {
  const files = Array.from(
    { length: CHANGED_LIMIT_PLUS_ONE },
    (_value, index) => `apps/core/state/src/file${String(index)}.rs`,
  );
  const report = buildReport(input({ changedFiles: files }));
  assert.equal(report.sourceFiles, CHANGED_LIMIT_PLUS_ONE);
  assert.equal(report.changedFiles.length, report.changedFileListingLimit);
});

void test("the cycles list is capped and its truncation is recorded", () => {
  const cycles = Array.from({ length: MAX_LISTED_CYCLES + 1 }, (_value, index) => [
    `apps/core/state/src/file${String(index)}.rs`,
    "apps/core/state/src/javas.rs",
  ]);
  const report = buildReport(
    input({ changedFiles: ["apps/core/state/src/javas.rs"], cycles }),
  );
  assert.equal(report.touchedCycleCount, MAX_LISTED_CYCLES + 1);
  assert.equal(report.touchedCycleFileCount, cycles.length + 1);
  assert.equal(report.cycles.length, MAX_LISTED_CYCLES);
  assert.equal(report.truncatedCycles, true);
});

void test("cycle group membership and file counts do not depend on rotation", () => {
  const files = [
    "apps/core/state/src/files.rs",
    "apps/core/state/src/javas.rs",
    "apps/core/state/src/profile.rs",
  ];
  const original = buildReport(input({ changedFiles: files, cycles: [files] }));
  const rotated = buildReport(input({
    changedFiles: files,
    cycles: [[...files.slice(1), ...files.slice(0, 1)]],
  }));
  assert.deepEqual(rotated.cycles, original.cycles);
  assert.equal(rotated.touchedCycleFileCount, files.length);
});

void test("a change without source files says so instead of implying a clean result", () => {
  const report = buildReport(
    input({ changedFiles: ["CONTRIBUTING.md"], affectedTests: [] }),
  );
  assert.equal(report.sourceFiles, 0);
  assert.match(report.note ?? "", /No changed file is indexed source code/);
});

void test("a partially parsed index is surfaced in the note", () => {
  const report = buildReport(
    input({
      index: {
        fileCount: INDEXED_FILES,
        nodeCount: NODE_COUNT,
        languages: ["rust"],
        filesWithParseErrors: 2,
      },
    }),
  );
  assert.match(report.note ?? "", /2 indexed file\(s\) failed to parse/);
});

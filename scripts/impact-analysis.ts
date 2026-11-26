// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

// Builds the pull-request impact report from CodeGraph results.
//
// The analysis job reads the changed paths, the affected tests and the import
// cycles from CodeGraph, then shapes them here. This module is pure, so the
// comment job renders the same model and a test can exercise the whole
// transformation without the tool.

import type {
  ChangedFile,
  ChangedFileKind,
  ImpactReport,
  TestGroup,
} from "./impact-report.ts";

/// How deep `codegraph affected` follows imports. A mirrored test file is one
/// import away from its source, and one more reaches a module-level test block
/// that names the source, so five is ample while keeping the search finite.
export const TRAVERSAL_DEPTH = 5;

/// Tests listed for one group before the rest are summarised by count.
export const MAX_LISTED_TESTS = 10;

/// Cycles listed in the comment.
export const MAX_LISTED_CYCLES = 5;

const MAX_CHANGED_FILES = 40;

const TESTS_DIRECTORY = "tests";
const TEST_ROOT = `${TESTS_DIRECTORY}/`;
const APPS_ROOT = "apps";
const CORE_GROUP = "core";
const LIB_DIRECTORY = "lib";
const SOURCE_DIRECTORY = "src";

/// Positional parts of a path, named where the index arithmetic matters.
const SEGMENT = {
  root: 0,
  first: 1,
  second: 2,
  third: 3,
  fourth: 4,
} as const;

const SOURCE_EXTENSIONS = [
  ".rs",
  ".ts",
  ".tsx",
  ".js",
  ".jsx",
  ".mjs",
  ".cjs",
  ".vue",
  ".svelte",
] as const;

/// Sort keys that place mirrored test groups before dependent ones.
const MIRRORED_FIRST = -1;
const DEPENDENT_FIRST = 1;

/// How many segments after the app prefix a crate directory starts.
const CRATE_OFFSET = {
  underApps: 1,
  underGroup: 2,
  underLib: 3,
} as const;

/// The files CodeGraph reports as test files that the repository does not keep
/// under `tests/`. They are still affected, and still worth naming.
const GENERATED_DIRECTORIES = ["node_modules/", "target/", "dist/"] as const;

export interface AnalysisContext {
  readonly repository: string;
  readonly pullRequest: number;
  readonly headSha: string;
  readonly baseSha: string;
  readonly analyzedAt: string;
  readonly analyzer: string;
  readonly traversalDepth: number;
}

/// The fields the report uses from `codegraph status --json`.
export interface IndexSummary {
  readonly fileCount: number;
  readonly nodeCount: number;
  readonly languages: readonly string[];
  readonly filesWithParseErrors: number;
}

/// One import cycle as `codegraph cycles --json` reports it.
export type Cycle = readonly string[];

export interface BuildReportInput {
  readonly context: AnalysisContext;
  readonly changedFiles: readonly string[];
  readonly affectedTests: readonly string[];
  readonly cycles: readonly Cycle[];
  readonly index: IndexSummary;
  readonly note: string | null;
}

function isSourceFile(path: string): boolean {
  return SOURCE_EXTENSIONS.some((extension) => path.endsWith(extension));
}

/// The directory a file sits in, with its trailing slash.
export function directoryOf(path: string): string {
  const separator = path.lastIndexOf("/");
  return separator < 0 ? "" : path.slice(0, separator + 1);
}

/// Only a code file drives the test and cycle analysis: documentation,
/// packaging, manifests and lockfiles are counted but never guessed at.
export function changedFileKind(path: string): ChangedFileKind {
  if (!isSourceFile(path)) {
    return "other";
  }
  return path.startsWith(TEST_ROOT) ? "test" : "source";
}

/// The crate directory a file under `apps/` or `tests/apps/` belongs to, such
/// as `apps/core/state`. `lib` only groups the crate sources, so it is not part
/// of the directory; `src`, `tests`, `examples` and `benches` mark where the
/// crate's own tree begins.
export function crateDirectory(path: string): string | undefined {
  const segments = path.split("/");
  const prefixLength = segments[SEGMENT.root] === TESTS_DIRECTORY ? SEGMENT.first : 0;
  if (segments[prefixLength] !== APPS_ROOT) {
    return undefined;
  }
  const crateIndex = prefixLength + crateOffset(segments, prefixLength);
  if (segments[crateIndex] === undefined) {
    return undefined;
  }
  const end = segments.findIndex(
    (segment, index) => index > crateIndex && CRATE_TREE_MARKERS.has(segment),
  );
  const last = end < 0 ? segments.length : end;
  return segments.slice(0, last).join("/");
}

/// Where the crate directory starts, relative to the `apps` segment.
/// `apps/<app>` is the crate itself. Under `apps/core`, `lib/<crate>` nests one
/// level deeper, while `apps/core/src` belongs to the umbrella `core` crate
/// rather than to a crate named `src`.
function crateOffset(segments: readonly string[], appsIndex: number): number {
  if (segments[appsIndex + SEGMENT.first] !== CORE_GROUP) {
    return CRATE_OFFSET.underApps;
  }
  const group = segments[appsIndex + SEGMENT.second];
  const followedBySource = segments[appsIndex + SEGMENT.third] === SOURCE_DIRECTORY;
  if (group === LIB_DIRECTORY && !followedBySource) {
    return CRATE_OFFSET.underLib;
  }
  if (group === SOURCE_DIRECTORY) {
    return CRATE_OFFSET.underApps;
  }
  return CRATE_OFFSET.underGroup;
}

/// Trees inside a crate that hold its own code or tests.
const CRATE_TREE_MARKERS = new Set(["src", "tests", "examples", "benches"]);

/// The `module:` label from `scripts/pull-request-labels.ts` for a path.
export function moduleOf(path: string): string {
  const crate = crateDirectory(path);
  if (crate !== undefined) {
    return crate.slice(crate.lastIndexOf("/") + 1);
  }
  if (path.startsWith("platforms/")) {
    return "platforms";
  }
  return "repository";
}

/// The crate a mirrored test belongs to, such as `apps/core/state`.
export function mirroredCrate(path: string): string | undefined {
  const crate = crateDirectory(path);
  return crate === undefined ? undefined : crate.slice(TEST_ROOT.length);
}

/// Groups affected tests by the crate they belong to. Mirrored tests come
/// first, because those are the files a contributor runs directly for the
/// change; the rest arrive through cross-crate dependencies.
export function groupTests(
  changedFiles: readonly string[],
  affectedTests: readonly string[],
): readonly TestGroup[] {
  /// Crates whose own tests mirror a changed file. When several changed files
  /// map to the same crate, the crate keeps the label of the first one, which
  /// is the same module label either way.
  const mirrors = new Map<string, string>();
  for (const changed of changedFiles) {
    const crate = crateDirectory(changed);
    if (crate !== undefined && !changed.startsWith(TEST_ROOT)) {
      mirrors.set(crate, moduleOf(changed));
    }
  }
  type Group = { module: string; mirrored: boolean; crate: string; tests: string[] };
  const groups = new Map<string, Group>();
  for (const test of affectedTests) {
    const crate = mirroredCrate(test);
    /// Only the test's own crate can mirror it: an umbrella crate such as
    /// `apps/core` does not own the tests of the crates nested under it.
    const mirror = crate !== undefined && mirrors.has(crate) ? crate : undefined;
    /// A crate groups its whole mirrored tree, dependencies included, so a
    /// crate cannot appear twice with different nested directories.
    /// A test outside the crates is grouped by its own directory, with no
    /// trailing slash and no duplication for a root-level file.
    const testDirectory = directoryOf(test).replace(/\/$/, "");
    const key = mirror ?? crate ?? (testDirectory.length === 0 ? test : testDirectory);
    const group = groups.get(key) ?? {
      module: mirror === undefined ? moduleOf(test) : (mirrors.get(mirror) ?? "repository"),
      mirrored: mirror !== undefined,
      crate: key,
      tests: [],
    };
    group.tests.push(test);
    groups.set(key, group);
  }
  return [...groups.values()]
    .map((group) => ({
      module: group.module,
      mirrored: group.mirrored,
      directory: group.mirrored ? `${group.crate.replace(/\/$/, "")}/` : group.crate,
      tests: [...group.tests].sort(),
    }))
    .sort((left, right) => {
      if (left.mirrored !== right.mirrored) {
        return left.mirrored ? MIRRORED_FIRST : DEPENDENT_FIRST;
      }
      if (left.tests.length !== right.tests.length) {
        return right.tests.length - left.tests.length;
      }
      return left.module.localeCompare(right.module) || left.directory.localeCompare(right.directory);
    });
}

function kindCountsOf(paths: readonly string[]): Readonly<Record<ChangedFileKind, number>> {
  return paths.reduce<Record<ChangedFileKind, number>>(
    (counts, path) => ({
      ...counts,
      [changedFileKind(path)]: counts[changedFileKind(path)] + 1,
    }),
    { source: 0, test: 0, other: 0 },
  );
}

/// A cycle is actionable when the pull request touches it and it runs between
/// files the repository maintains, rather than through a generated directory.
export function touchedCycles(
  cycles: readonly Cycle[],
  changedFiles: readonly string[],
): readonly Cycle[] {
  return cycles.filter(
    (cycle) =>
      cycle.every(
        (path) => !GENERATED_DIRECTORIES.some((directory) => path.startsWith(directory)),
      ) && cycle.some((path) => changedFiles.includes(path)),
  );
}

/// The number of touched cycles whose lowest-sorted file is this one.
function cyclesByFile(cycles: readonly Cycle[]): Readonly<Record<string, number>> {
  const counts: Record<string, number> = {};
  for (const cycle of cycles) {
    const [head] = [...cycle].sort();
    if (head !== undefined) {
      counts[head] = (counts[head] ?? 0) + 1;
    }
  }
  return counts;
}

/// Build the report from the CodeGraph answers and the pull-request context.
export function buildReport(input: BuildReportInput): ImpactReport {
  const kindCounts = kindCountsOf(input.changedFiles);
  const codeFiles = kindCounts.source + kindCounts.test;
  const cycles = touchedCycles(input.cycles, input.changedFiles);
  const notes = [
    input.note,
    codeFiles === 0
      ? "No changed file is indexed source code, so the test and cycle sections stay empty."
      : null,
    input.index.filesWithParseErrors > 0
      ? `${String(input.index.filesWithParseErrors)} indexed file(s) failed to parse, so the results can be incomplete.`
      : null,
  ].filter((note): note is string => note !== null);
  return {
    ...input.context,
    changedFileCount: input.changedFiles.length,
    sourceFiles: kindCounts.source,
    testFiles: kindCounts.test,
    otherFiles: kindCounts.other,
    changedFiles: input.changedFiles.slice(0, MAX_CHANGED_FILES).map(
      (path): ChangedFile => ({ path, kind: changedFileKind(path) }),
    ),
    changedFileListingLimit: MAX_CHANGED_FILES,
    affectedTestCount: input.affectedTests.length,
    testGroups: groupTests(input.changedFiles, input.affectedTests),
    testListingLimit: MAX_LISTED_TESTS,
    touchedCycleCount: cycles.length,
    cyclesByFile: cyclesByFile(cycles),
    cycleListingLimit: MAX_LISTED_CYCLES,
    cycles: cycles.slice(0, MAX_LISTED_CYCLES),
    index: {
      fileCount: input.index.fileCount,
      nodeCount: input.index.nodeCount,
      languages: input.index.languages,
      filesWithParseErrors: input.index.filesWithParseErrors,
    },
    truncatedCycles: cycles.length > MAX_LISTED_CYCLES,
    note: notes.join("\n\n") || null,
  };
}

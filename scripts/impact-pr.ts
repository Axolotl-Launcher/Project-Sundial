// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

// Builds the impact-report artifacts for a pull request.
//
// Run from the analysis job after `codegraph init` indexed the pull request as
// data. It reads the index through the CodeGraph CLI, then writes the report and
// its JSON form to an output directory. No GitHub API call happens here.

import { execFileSync } from "node:child_process";
import { mkdirSync, writeFileSync } from "node:fs";
import { join, resolve } from "node:path";

import {
  TRAVERSAL_DEPTH,
  buildReport,
  type AnalysisContext,
  type Cycle,
  type IndexSummary,
} from "./impact-analysis.ts";
import { renderComment } from "./impact-report.ts";

const ANALYZER = "codegraph@1.6.4";
const JSON_INDENT = 2;
const BYTES_PER_KIB = 1024;
const KIB_PER_MIB = 1024;
const MAX_OUTPUT_MIB = 64;
const MAX_OUTPUT_BYTES = MAX_OUTPUT_MIB * KIB_PER_MIB * BYTES_PER_KIB;

/// `codegraph cycles` reports a project that has cycles with this exit status.
const CYCLES_FOUND_STATUS = 1;

function required(name: string): string {
  const value = process.env[name];
  if (value === undefined || value.length === 0) {
    throw new Error(`${name} is not set`);
  }
  return value;
}

function git(args: readonly string[], cwd: string): Buffer {
  return execFileSync("git", args, {
    cwd,
    windowsHide: true,
    maxBuffer: MAX_OUTPUT_BYTES,
  });
}

/// The commit the pull request branched from, not the current base tip. A
/// two-dot diff against the tip would include base-branch drift.
function mergeBase(baseSha: string, headSha: string, cwd: string): string {
  return git(["merge-base", baseSha, headSha], cwd).toString("utf8").trim();
}

export function decodeChangedPaths(output: Uint8Array): readonly string[] {
  const decoder = new TextDecoder("utf-8", { fatal: true, ignoreBOM: true });
  return decoder.decode(output).split("\0").filter((path) => path.length > 0);
}

function changedPaths(base: string, head: string, worktree: string): readonly string[] {
  return decodeChangedPaths(
    git(["diff", "--name-only", "--no-renames", "-z", base, head, "--"], worktree),
  );
}

export function affectedInputPaths(paths: readonly string[]): readonly string[] {
  return paths.filter(
    (path) => path === path.trim() && !/[\r\n\\]/.test(path),
  );
}

function outputDirectory(): string {
  return resolve(required("IMPACT_OUTPUT"));
}

/// The workflow supplies the executable and its arguments: the npm shim on
/// Windows and the Linux launcher take different paths, and spawning either
/// through a shell would make argument handling differ.
function codegraphCommand(): { command: string; args: readonly string[] } {
  const parsed: unknown = JSON.parse(required("CODEGRAPH_ARGS"));
  if (
    !Array.isArray(parsed) ||
    !parsed.every((argument): argument is string => typeof argument === "string")
  ) {
    throw new Error("CODEGRAPH_ARGS must be a JSON array of strings");
  }
  return { command: required("CODEGRAPH_COMMAND"), args: parsed };
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null;
}

function numberField(record: Record<string, unknown>, name: string): number {
  const value = record[name];
  if (typeof value !== "number") {
    throw new Error(`codegraph status returned no numeric ${name}`);
  }
  return value;
}

function stringArray(value: unknown): readonly string[] {
  if (!Array.isArray(value)) {
    return [];
  }
  return value.filter((entry): entry is string => typeof entry === "string");
}

function asIndex(value: unknown): IndexSummary {
  if (!isRecord(value)) {
    throw new Error("codegraph status returned a non-object result");
  }
  return {
    fileCount: numberField(value, "fileCount"),
    nodeCount: numberField(value, "nodeCount"),
    languages: stringArray(value["languages"]),
    filesWithParseErrors: 0,
  };
}

function parseErrors(value: unknown): number {
  if (!isRecord(value) || !isRecord(value["index"])) {
    return 0;
  }
  const count = value["index"]["filesWithParseErrors"];
  return typeof count === "number" ? count : 0;
}

function asAffected(value: unknown): readonly string[] {
  if (!isRecord(value)) {
    throw new Error("codegraph affected returned a non-object result");
  }
  return stringArray(value["affectedTests"]);
}

function asCycles(value: unknown): readonly Cycle[] {
  if (!isRecord(value) || !Array.isArray(value["cycles"])) {
    throw new Error("codegraph cycles returned a non-object result");
  }
  return value["cycles"]
    .filter(Array.isArray)
    .map((cycle: readonly unknown[]) => stringArray(cycle))
    .filter((cycle) => cycle.length > 0);
}

function callCodegraph(
  invocation: { command: string; args: readonly string[] },
  worktree: string,
  args: readonly string[],
  input?: string,
): string {
  try {
    return execFileSync(invocation.command, [...invocation.args, ...args], {
      cwd: worktree,
      encoding: "utf8",
      input,
      windowsHide: true,
      maxBuffer: MAX_OUTPUT_BYTES,
    });
  } catch (error) {
    const failed = error as { status?: unknown; stdout?: unknown; stderr?: unknown };
    const stdout = typeof failed.stdout === "string" ? failed.stdout : "";
    const stderr = typeof failed.stderr === "string" ? failed.stderr : "";
    /// `codegraph cycles` exits 1 when the project has cycles, which is its
    /// documented gate behaviour and still a successful query.
    if (args[0] === "cycles" && failed.status === CYCLES_FOUND_STATUS) {
      return stdout;
    }
    throw new Error(
      `codegraph ${args.join(" ")} failed: ${stderr.trim() || (error instanceof Error ? error.message : String(error))}`,
      { cause: error },
    );
  }
}

function writeOutputs(report: ReturnType<typeof buildReport>): void {
  const directory = outputDirectory();
  mkdirSync(directory, { recursive: true });
  writeFileSync(join(directory, "impact.json"), `${JSON.stringify(report, null, JSON_INDENT)}\n`);
  writeFileSync(join(directory, "impact.md"), `${renderComment(report)}\n`);
}

function printSummary(report: ReturnType<typeof buildReport>): void {
  console.log(
    `Impact report: ${String(report.changedFileCount)} changed files, ${String(report.affectedTestCount)} affected tests, ${String(report.touchedCycleCount)} touched cycles`,
  );
}

interface GraphAnswers {
  readonly index: IndexSummary;
  readonly affectedTests: readonly string[];
  readonly affectedLookupOmitted: number;
  readonly cycles: readonly Cycle[];
}

/// A failure here aborts the analysis instead of publishing an empty report:
/// "no affected tests" is a plausible-looking lie when the graph was never
/// queried, and an advisory comment must not claim more than it checked.
function queryGraph(
  invocation: { command: string; args: readonly string[] },
  worktree: string,
  files: readonly string[],
): GraphAnswers {
  const status = JSON.parse(callCodegraph(invocation, worktree, ["status", "--json"])) as unknown;
  const inputPaths = affectedInputPaths(files);
  const affected = inputPaths.length === 0 ? [] : asAffected(
    JSON.parse(
      callCodegraph(
        invocation,
        worktree,
        ["affected", "--stdin", "--json", "--depth", String(TRAVERSAL_DEPTH)],
        `${inputPaths.join("\n")}\n`,
      ),
    ) as unknown,
  );
  const cycles = asCycles(
    JSON.parse(callCodegraph(invocation, worktree, ["cycles", "--json"])) as unknown,
  );
  return {
    index: { ...asIndex(status), filesWithParseErrors: parseErrors(status) },
    affectedTests: affected,
    affectedLookupOmitted: files.length - inputPaths.length,
    cycles,
  };
}

function analyze(): number {
  const repository = required("GITHUB_REPOSITORY");
  const pullRequest = Number(required("PR_NUMBER"));
  const headSha = required("HEAD_SHA");
  const baseSha = required("BASE_SHA");
  const worktree = resolve(required("IMPACT_WORKTREE"));
  const mergeBaseSha = mergeBase(baseSha, headSha, worktree);
  const files = changedPaths(mergeBaseSha, headSha, worktree);
  const context: AnalysisContext = {
    repository,
    pullRequest,
    headSha,
    baseSha: mergeBaseSha,
    analyzedAt: new Date().toISOString(),
    analyzer: ANALYZER,
    traversalDepth: TRAVERSAL_DEPTH,
  };
  const answers = queryGraph(codegraphCommand(), worktree, files);

  const report = buildReport({
    context,
    changedFiles: files,
    ...answers,
    note: null,
  });
  writeOutputs(report);
  printSummary(report);
  return 0;
}

if (import.meta.main) {
  try {
    process.exitCode = analyze();
  } catch (error) {
    console.error(error instanceof Error ? error.message : String(error));
    process.exitCode = 1;
  }
}

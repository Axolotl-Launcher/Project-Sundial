// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

// Applies `module:`, `version:` and `status: unreviewed` labels to a pull
// request, based on the paths it changes.
//
// The changed files come from the GitHub API rather than a checkout: the
// workflow runs on `pull_request_target`, where the checked-out tree is the
// base branch and a diff against it would describe the wrong thing.

import { execFileSync } from "node:child_process";

import { modulesForPaths } from "./pull-request-labels.ts";

/// The repository the workflow runs in, e.g. `owner/name`.
const REPOSITORY = process.env["GITHUB_REPOSITORY"];

function changedPaths(number: string): string[] {
  const output = execFileSync(
    "gh",
    ["api", `repos/${REPOSITORY ?? ""}/pulls/${number}/files`, "--paginate", "--jq", ".[].filename"],
    { encoding: "utf8" },
  );
  return output.split("\n").filter((line) => line.length > 0);
}

/// Reads the workspace version from the repository root manifest.
function workspaceVersion(): string | undefined {
  const output = execFileSync("git", ["show", "HEAD:Cargo.toml"], {
    encoding: "utf8",
  });
  const match = /^\s*version\s*=\s*"([^"]+)"/m.exec(output);
  return match?.[1];
}

function apply(number: string, labels: readonly string[]): void {
  if (labels.length === 0) {
    return;
  }
  execFileSync(
    "gh",
    ["pr", "edit", number, ...labels.flatMap((label) => ["--add-label", label])],
    { stdio: "inherit" },
  );
}

function main(): number {
  const number = process.env["PR_NUMBER"];
  if (number === undefined) {
    console.error("PR_NUMBER is not set");
    return 1;
  }

  const modules = modulesForPaths(changedPaths(number));
  const version = workspaceVersion();

  const labels = [
    ...modules.map((module) => `module: ${module}`),
    ...(version === undefined ? [] : [`version: ${version}`]),
    "status: unreviewed",
  ];

  apply(number, labels);
  return 0;
}

process.exit(main());

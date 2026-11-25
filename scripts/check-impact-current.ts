// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

// Prevents an older analysis run from overwriting a newer impact comment.

import { execFileSync } from "node:child_process";
import { appendFileSync } from "node:fs";
import { join, resolve } from "node:path";

import { assertInteger, assertString, isRecord, readArtifact } from "./impact-artifact.ts";

interface PullRequest {
  readonly head_sha?: string;
}

function required(name: string): string {
  const value = process.env[name];
  if (value === undefined || value.length === 0) {
    throw new Error(`${name} is not set`);
  }
  return value;
}

function main(): number {
  const directory = resolve(required("IMPACT_ARTIFACT_DIR"));
  const artifact = readArtifact(join(directory, "impact.json"));
  if (!isRecord(artifact)) {
    throw new Error("impact.json is not an object");
  }
  const pullRequest = assertInteger(artifact["pullRequest"], "pullRequest");
  const headSha = assertString(artifact["headSha"], "headSha");
  const output = execFileSync(
    "gh",
    [
      "api",
      `repos/${required("GITHUB_REPOSITORY")}/pulls/${String(pullRequest)}`,
      "--jq",
      "{head_sha: .head.sha}",
    ],
    {
      encoding: "utf8",
      windowsHide: true,
      env: { ...process.env, GH_TOKEN: required("COMMENT_TOKEN") },
    },
  );
  const currentPullRequest = JSON.parse(output) as PullRequest;
  const current = currentPullRequest.head_sha === headSha;
  const summary = process.env["GITHUB_OUTPUT"];
  if (summary !== undefined) {
    appendFileSync(summary, `current=${String(current)}\n`);
  }
  if (!current) {
    console.log(
      `Skipping superseded analysis for ${headSha.slice(0, 12)}; pull request is at ${currentPullRequest.head_sha?.slice(0, 12) ?? "?"}`,
    );
  }
  return 0;
}

try {
  process.exit(main());
} catch (error) {
  console.error(error instanceof Error ? error.message : String(error));
  process.exit(1);
}

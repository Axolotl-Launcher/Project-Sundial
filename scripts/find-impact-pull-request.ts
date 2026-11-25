// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

// Resolves and verifies the pull request for an analysis workflow run.
//
// `workflow_run` does not expose `pull_request.number` on the top-level event,
// so this reads the run's associated pull requests from the Actions API and
// checks the artifact's head SHA against that pull request. An artifact alone
// is never trusted to choose where a comment is posted.

import { execFileSync } from "node:child_process";
import { mkdirSync, writeFileSync } from "node:fs";
import { join, resolve } from "node:path";

import { assertInteger, assertString, isRecord, readArtifact } from "./impact-artifact.ts";

interface Run {
  readonly event?: string;
}

interface PullRequestReference {
  readonly number?: number;
}

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

function assertAssociated(
  environment: NodeJS.ProcessEnv,
  repository: string,
  runId: string,
  pullRequest: number,
): void {
  const associatedOutput = execFileSync(
    "gh",
    [
      "api",
      `repos/${repository}/actions/runs/${runId}/pull_requests`,
      "--jq",
      "[.[].number]",
    ],
    { encoding: "utf8", windowsHide: true, env: environment },
  );
  const associated = JSON.parse(associatedOutput) as readonly number[];
  if (!associated.includes(pullRequest)) {
    throw new Error(`pull request #${String(pullRequest)} is not associated with run ${runId}`);
  }
}

function assertHead(environment: NodeJS.ProcessEnv, repository: string, pullRequest: number, headSha: string): void {
  const output = execFileSync(
    "gh",
    [
      "api",
      `repos/${repository}/pulls/${String(pullRequest)}`,
      "--jq",
      "{head_sha: .head.sha}",
    ],
    { encoding: "utf8", windowsHide: true, env: environment },
  );
  const current = JSON.parse(output) as PullRequest;
  if (current.head_sha !== headSha) {
    throw new Error(`artifact head ${headSha} does not match pull request #${String(pullRequest)} head ${current.head_sha ?? "?"}`);
  }
}

function main(): number {
  const runId = required("ANALYSIS_RUN_ID");
  const repository = required("GITHUB_REPOSITORY");
  const environment = { ...process.env, GH_TOKEN: required("COMMENT_TOKEN") };
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
      `repos/${repository}/actions/runs/${runId}`,
      "--jq",
      "{event}",
    ],
    { encoding: "utf8", windowsHide: true, env: environment },
  );
  const run = JSON.parse(output) as Run;
  if (run.event !== "pull_request") {
    throw new Error(`run ${runId} was not triggered by a pull request`);
  }
  assertAssociated(environment, repository, runId, pullRequest);
  assertHead(environment, repository, pullRequest, headSha);
  mkdirSync(directory, { recursive: true });
  writeFileSync(
    join(directory, "pull-request.json"),
    `${JSON.stringify({ number: pullRequest } satisfies PullRequestReference)}\n`,
  );
  console.log(`Impact comment targets pull request #${String(pullRequest)} at ${headSha.slice(0, 12)}`);
  return 0;
}

try {
  process.exit(main());
} catch (error) {
  console.error(error instanceof Error ? error.message : String(error));
  process.exit(1);
}

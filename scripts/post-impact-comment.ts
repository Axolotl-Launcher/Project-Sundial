// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

// Publishes the impact report on a pull request.
//
// The report is produced by the trusted base-branch analysis in the same
// workflow run, so this script receives the final Markdown directly. It never
// reads or executes anything from the pull request. A hidden marker keeps the
// comment idempotent: later runs update the existing comment.

import { execFileSync } from "node:child_process";

import { REPORT_MARKER } from "./impact-report.ts";

const MAX_COMMENT_LENGTH = 60_000;

function required(name: string): string {
  const value = process.env[name];
  if (value === undefined || value.length === 0) {
    throw new Error(`${name} is not set`);
  }
  return value;
}

/// Picks the existing marker comment, if any, so reruns update it.
function existingCommentId(
  gh: (args: readonly string[]) => string,
  repository: string,
  pullRequest: number,
): string | undefined {
  const output = gh([
    "api",
    `repos/${repository}/issues/${String(pullRequest)}/comments`,
    "--paginate",
    "--jq",
    `.[] | select(.body | startswith(${JSON.stringify(REPORT_MARKER)})) | .id`,
  ]);
  const id = output.split("\n").find((line) => line.trim().length > 0);
  return id?.trim();
}

function publish(
  repository: string,
  pullRequest: number,
  body: string,
  token: string,
): void {
  if (body.length > MAX_COMMENT_LENGTH) {
    throw new Error(
      `impact comment is ${String(body.length)} characters; limit is ${String(MAX_COMMENT_LENGTH)}`,
    );
  }
  const environment = { ...process.env, GH_TOKEN: token };
  const ghWithToken = (args: readonly string[], body?: string): string =>
    execFileSync("gh", args, {
      encoding: "utf8",
      input: body,
      windowsHide: true,
      env: environment,
    });
  const commentId = existingCommentId(ghWithToken, repository, pullRequest);
  if (commentId === undefined) {
    ghWithToken(
      [
        "api",
        "--method",
        "POST",
        `repos/${repository}/issues/${String(pullRequest)}/comments`,
        "--input",
        "-",
      ],
      JSON.stringify({ body }),
    );
    return;
  }
  ghWithToken(
    [
      "api",
      "--method",
      "PATCH",
      `repos/${repository}/issues/comments/${commentId}`,
      "--input",
      "-",
    ],
    JSON.stringify({ body }),
  );
}

function main(): number {
  const markdown = required("IMPACT_MARKDOWN");
  if (!markdown.startsWith(REPORT_MARKER)) {
    throw new Error("the analysis output is missing its report marker");
  }
  const repository = required("GITHUB_REPOSITORY");
  const pullRequest = Number(required("PR_NUMBER"));
  if (!Number.isInteger(pullRequest) || pullRequest <= 0) {
    throw new Error("PR_NUMBER is not a pull request number");
  }
  publish(repository, pullRequest, markdown, required("GH_TOKEN"));
  return 0;
}

try {
  process.exit(main());
} catch (error) {
  console.error(error instanceof Error ? error.message : String(error));
  process.exit(1);
}

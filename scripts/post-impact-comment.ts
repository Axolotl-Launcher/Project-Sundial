// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

// Publishes the impact report on a pull request.
//
// This runs in a workflow with `pull-requests: write` but checks out the base
// branch only. It validates the uploaded artifact before using it, and never
// executes anything from the pull request. A hidden marker keeps the comment
// idempotent: later runs update the existing comment.

import { execFileSync } from "node:child_process";
import { resolve } from "node:path";

import {
  assertBoolean,
  assertInteger,
  assertNumber,
  assertString,
  isRecord,
  readArtifact,
} from "./impact-artifact.ts";
import {
  REPORT_MARKER,
  renderComment,
  type ImpactReport,
} from "./impact-report.ts";

const MAX_COMMENT_LENGTH = 60_000;
const RISK_LEVELS = new Set(["none", "low", "medium", "high", "critical", "unknown"]);

function required(name: string): string {
  const value = process.env[name];
  if (value === undefined || value.length === 0) {
    throw new Error(`${name} is not set`);
  }
  return value;
}

function riskField(value: unknown, field: string): ImpactReport["risk"] {
  const risk = assertString(value, field);
  if (!RISK_LEVELS.has(risk)) {
    throw new Error(`impact.json has an unknown ${field} "${risk}"`);
  }
  return risk as ImpactReport["risk"];
}

function reportHeader(parsed: Record<string, unknown>): Omit<ImpactReport, "symbols" | "processes" | "upstream"> {
  return {
    repository: assertString(parsed["repository"], "repository"),
    pullRequest: assertInteger(parsed["pullRequest"], "pullRequest"),
    headSha: assertString(parsed["headSha"], "headSha"),
    baseSha: assertString(parsed["baseSha"], "baseSha"),
    analyzedAt: assertString(parsed["analyzedAt"], "analyzedAt"),
    analyzer: assertString(parsed["analyzer"], "analyzer"),
    risk: riskField(parsed["risk"], "risk"),
    changedFiles: assertNumber(parsed["changedFiles"], "changedFiles"),
    changedSymbols: assertNumber(parsed["changedSymbols"], "changedSymbols"),
    affectedProcesses: assertNumber(
      parsed["affectedProcesses"],
      "affectedProcesses",
    ),
    truncated: assertBoolean(parsed["truncated"], "truncated"),
    partial: assertBoolean(parsed["partial"], "partial"),
    note: parsed["note"] === null ? null : assertString(parsed["note"], "note"),
  };
}

function loadReport(path: string): ImpactReport {
  const parsed = readArtifact(path);
  if (!isRecord(parsed)) {
    throw new Error("impact.json is not an object");
  }
  return {
    ...reportHeader(parsed),
    symbols: arrayField(parsed["symbols"]).map((symbol) => {
      if (!isRecord(symbol)) {
        throw new Error("impact.json has a malformed symbol");
      }
      return {
        name: assertString(symbol["name"], "symbols[].name"),
        type: assertString(symbol["type"], "symbols[].type"),
        path: assertString(symbol["path"], "symbols[].path"),
      };
    }),
    processes: arrayField(parsed["processes"]).map((process) => {
      if (!isRecord(process)) {
        throw new Error("impact.json has a malformed process");
      }
      const steps = Array.isArray(process["changedSteps"])
        ? process["changedSteps"].map((step) => assertString(step, "changedSteps[]"))
        : [];
      return {
        name: assertString(process["name"], "processes[].name"),
        changedSteps: steps,
        stepCount: assertNumber(process["stepCount"], "processes[].stepCount"),
      };
    }),
    upstream: arrayField(parsed["upstream"]).map((caller) => {
      if (!isRecord(caller)) {
        throw new Error("impact.json has a malformed upstream caller");
      }
      return {
        name: assertString(caller["name"], "upstream[].name"),
        risk: riskField(caller["risk"], "upstream[].risk"),
        direct: assertNumber(caller["direct"], "upstream[].direct"),
        affectedProcesses: assertNumber(
          caller["affectedProcesses"],
          "upstream[].affectedProcesses",
        ),
      };
    }),
  };
}

function arrayField(value: unknown): readonly unknown[] {
  if (!Array.isArray(value)) {
    throw new Error("impact.json is missing an array field");
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
  const reportPath = resolve(
    required("IMPACT_ARTIFACT_DIR"),
    "impact.json",
  );
  const report = loadReport(reportPath);
  const pullRequest = readArtifact(
    resolve(required("IMPACT_ARTIFACT_DIR"), "pull-request.json"),
  );
  if (!isRecord(pullRequest)) {
    throw new Error("pull-request.json is not an object");
  }
  const target = assertInteger(pullRequest["number"], "pull-request number");
  const repository = required("GITHUB_REPOSITORY");
  const token = required("COMMENT_TOKEN");
  const runUrl = process.env["ANALYSIS_RUN_URL"];
  if (report.repository !== repository) {
    throw new Error(
      `impact.json belongs to ${report.repository}, not ${repository}`,
    );
  }
  if (target !== report.pullRequest) {
    throw new Error(
      `pull-request.json (${String(target)}) and impact.json (${String(report.pullRequest)}) disagree`,
    );
  }
  const artifactLink =
    runUrl === undefined
      ? ""
      : `\n\nThe draw.io mind map, Mermaid source and SVG preview are in the [\`pr-impact\` artifact](${runUrl}#artifacts) for this run.`;
  publish(repository, target, `${renderComment(report)}${artifactLink}`, token);
  return 0;
}

try {
  process.exit(main());
} catch (error) {
  console.error(error instanceof Error ? error.message : String(error));
  process.exit(1);
}

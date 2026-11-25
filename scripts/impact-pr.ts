// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

// Builds the impact-report artifacts for a pull request.
//
// Run from the analysis job. It expects GitNexus to be installed in the
// temporary CI home, and writes the report plus its Mermaid, draw.io and SVG
// renderings to an output directory. No GitHub API call happens here.

import { execFileSync } from "node:child_process";
import { mkdirSync, writeFileSync } from "node:fs";
import { join, resolve } from "node:path";

import {
  impactTargets,
  type DetectChangesInput,
  type ImpactInput,
  buildReport,
} from "./impact-analysis.ts";
import { GitNexusClient } from "./impact-mcp.ts";
import { renderMermaid, renderComment } from "./impact-report.ts";
import { renderDrawio, renderSvg } from "./impact-mindmap.ts";

const ANALYZER = "gitnexus@1.6.12";
const JSON_INDENT = 2;

function git(args: readonly string[]): string {
  return execFileSync("git", args, { encoding: "utf8", windowsHide: true }).trim();
}

function required(name: string): string {
  const value = process.env[name];
  if (value === undefined || value.length === 0) {
    throw new Error(`${name} is not set`);
  }
  return value;
}

/// The commit the pull request branched from, not the current base tip. A
/// two-dot diff against the tip would include base-branch drift.
function mergeBase(baseSha: string, headSha: string): string {
  return git(["merge-base", baseSha, headSha]);
}

function outputDirectory(): string {
  return resolve(required("IMPACT_OUTPUT"));
}

function writeOutputs(report: ReturnType<typeof buildReport>): void {
  const directory = outputDirectory();
  mkdirSync(directory, { recursive: true });
  writeFileSync(join(directory, "impact.json"), `${JSON.stringify(report, null, JSON_INDENT)}\n`);
  writeFileSync(join(directory, "impact.md"), `${renderComment(report)}\n`);
  writeFileSync(join(directory, "impact.mmd"), renderMermaid(report));
  writeFileSync(join(directory, "impact.drawio"), renderDrawio(report));
  writeFileSync(join(directory, "impact.svg"), renderSvg(report));
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null;
}

function asDetectChanges(value: unknown): DetectChangesInput {
  return isRecord(value) ? value : {};
}

function asImpact(value: unknown): ImpactInput {
  return isRecord(value) ? value : {};
}

function printSummary(report: ReturnType<typeof buildReport>): void {
  console.log(
    `Impact report: ${report.risk} risk, ${String(report.changedSymbols)} symbols, ${String(report.affectedProcesses)} processes`,
  );
}

/// The workflow supplies the executable and its arguments explicitly: npm
/// installs a `.cmd` shim on Windows, while Linux has a shebang script, and
/// spawning either through a shell would make argument handling differ.
function mcpCommand(): { command: string; args: readonly string[] } {
  const command = required("GITNEXUS_MCP_COMMAND");
  const rawArgs = required("GITNEXUS_MCP_ARGS");
  const parsed: unknown = JSON.parse(rawArgs);
  if (
    !Array.isArray(parsed) ||
    !parsed.every((argument): argument is string => typeof argument === "string")
  ) {
    throw new Error("GITNEXUS_MCP_ARGS must be a JSON array of strings");
  }
  return { command, args: parsed };
}

async function detectChanges(
  client: GitNexusClient,
  mergeBaseSha: string,
): Promise<DetectChangesInput> {
  return asDetectChanges(
    await client.callTool("detect_changes", {
      scope: "compare",
      base_ref: mergeBaseSha,
      repo: ".",
    }),
  );
}

async function collectImpacts(
  client: GitNexusClient,
  detect: DetectChangesInput,
): Promise<ImpactInput[]> {
  const impacts: ImpactInput[] = [];
  for (const target of impactTargets(detect)) {
    try {
      impacts.push(
        {
          ...asImpact(
            await client.callTool("impact", {
              target: target.name,
              ...(target.path === undefined ? {} : { file_path: target.path }),
              direction: "upstream",
              repo: ".",
              summaryOnly: true,
            }),
          ),
          target: { name: target.name },
        },
      );
    } catch (error) {
      console.warn(
        `Impact failed for ${target.name}: ${error instanceof Error ? error.message : String(error)}`,
      );
    }
  }
  return impacts;
}

async function analyze(): Promise<number> {
  const repository = required("GITHUB_REPOSITORY");
  const pullRequest = Number(required("PR_NUMBER"));
  const headSha = required("HEAD_SHA");
  const baseSha = required("BASE_SHA");
  const worktree = process.cwd();
  const mergeBaseSha = mergeBase(baseSha, headSha);
  const invocation = mcpCommand();
  const client = new GitNexusClient(
    invocation.command,
    invocation.args,
    worktree,
    {
      ...process.env,
      GITNEXUS_HOME: required("GITNEXUS_HOME"),
    },
  );

  try {
    await client.initialize();
    const detect = await detectChanges(client, mergeBaseSha);
    const impacts = await collectImpacts(client, detect);
    const report = buildReport(
      {
        repository,
        pullRequest,
        headSha,
        baseSha: mergeBaseSha,
        analyzedAt: new Date().toISOString(),
        analyzer: ANALYZER,
      },
      detect,
      impacts,
    );
    writeOutputs(report);
    printSummary(report);
    return 0;
  } finally {
    client.close();
  }
}

try {
  process.exit(await analyze());
} catch (error) {
  console.error(error instanceof Error ? error.message : String(error));
  process.exit(1);
}

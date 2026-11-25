// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

// Turns GitNexus tool results into the stable impact-report model.

import {
  highestRisk,
  type AffectedProcess,
  type ChangedSymbol,
  type ImpactReport,
  type RiskLevel,
  type UpstreamCaller,
} from "./impact-report.ts";

export interface DetectChangesInput {
  readonly summary?: {
    readonly changed_count?: number;
    readonly affected_count?: number;
    readonly changed_files?: number;
    readonly risk_level?: string;
    readonly message?: string;
  };
  readonly changed_symbols?: readonly {
    readonly name?: string;
    readonly type?: string;
    readonly filePath?: string;
  }[];
  readonly affected_processes?: readonly {
    readonly name?: string;
    readonly step_count?: number;
    readonly changed_steps?: readonly { readonly symbol?: string }[];
  }[];
  readonly partial?: boolean;
  readonly truncated?: boolean;
  readonly error?: string;
}

export interface ImpactInput {
  readonly target?: { readonly name?: string };
  readonly risk?: string;
  readonly epistemic?: string;
  readonly error?: string;
  readonly partial?: boolean;
  readonly summary?: {
    readonly direct?: number;
    readonly processes_affected?: number;
  };
}

export interface AnalysisContext {
  readonly repository: string;
  readonly pullRequest: number;
  readonly headSha: string;
  readonly baseSha: string;
  readonly analyzedAt: string;
  readonly analyzer: string;
}

const MAX_CHANGED_SYMBOLS = 30;
const MAX_AFFECTED_PROCESSES = 20;
const MAX_IMPACT_TARGETS = 10;
const IMPACT_KINDS = new Set([
  "Function",
  "Method",
  "Class",
  "Interface",
  "Struct",
  "Enum",
  "Trait",
  "Const",
]);

const RISK_LEVELS: readonly RiskLevel[] = [
  "none",
  "low",
  "medium",
  "high",
  "critical",
  "unknown",
];

export function normalizeRisk(value: string | undefined): RiskLevel {
  const normalized = value?.trim().toLowerCase();
  return RISK_LEVELS.find((risk) => risk === normalized) ?? "unknown";
}

function symbolsFrom(input: DetectChangesInput): ChangedSymbol[] {
  return (input.changed_symbols ?? [])
    .slice(0, MAX_CHANGED_SYMBOLS)
    .map((symbol) => ({
      name: symbol.name ?? "?",
      type: symbol.type ?? "?",
      path: symbol.filePath ?? "?",
    }));
}

function processesFrom(input: DetectChangesInput): AffectedProcess[] {
  return (input.affected_processes ?? [])
    .slice(0, MAX_AFFECTED_PROCESSES)
    .map((process) => ({
      name: process.name ?? "?",
      stepCount: process.step_count ?? 0,
      changedSteps: (process.changed_steps ?? [])
        .map((step) => step.symbol)
        .filter((symbol): symbol is string => symbol !== undefined),
    }));
}

/// The change result's risk includes process count. A per-symbol lookup that
/// fails is advisory: it can raise the risk but must not downgrade the full
/// change analysis to "unknown". A successful `unknown` lookup is also not a
/// verdict for the whole change: it applies to that symbol and is reported in
/// the upstream table instead.
export function impactRisk(
  detected: DetectChangesInput,
  impacts: readonly ImpactInput[],
): RiskLevel {
  if (detected.partial === true) {
    return "unknown";
  }
  const risks = [normalizeRisk(detected.summary?.risk_level)];
  for (const impact of impacts) {
    if (
      impact.partial !== true &&
      impact.error === undefined &&
      impact.risk !== undefined &&
      normalizeRisk(impact.risk) !== "unknown"
    ) {
      risks.push(normalizeRisk(impact.risk));
    }
  }
  return highestRisk(risks);
}

function upstreamFrom(impacts: readonly ImpactInput[]): UpstreamCaller[] {
  return impacts.flatMap((impact) => {
    if (
      impact.error !== undefined ||
      impact.partial === true ||
      impact.target?.name === undefined ||
      (impact.summary?.direct ?? 0) === 0
    ) {
      return [];
    }
    return [
      {
        name: impact.target.name,
        risk: normalizeRisk(impact.risk),
        direct: impact.summary?.direct ?? 0,
        affectedProcesses: impact.summary?.processes_affected ?? 0,
      },
    ];
  });
}

/// Build the report from the GitNexus results and the pull-request context.
export function buildReport(
  context: AnalysisContext,
  detected: DetectChangesInput,
  impacts: readonly ImpactInput[],
): ImpactReport {
  const risk = impactRisk(detected, impacts);
  const note = detected.error ?? detected.summary?.message ?? null;
  const symbols = symbolsFrom(detected);
  const processes = processesFrom(detected);
  const partial = detected.partial === true;
  const truncated =
    detected.truncated === true ||
    (detected.summary?.changed_count ?? 0) > MAX_CHANGED_SYMBOLS ||
    (detected.summary?.affected_count ?? 0) > MAX_AFFECTED_PROCESSES ||
    (detected.changed_symbols?.length ?? 0) > MAX_CHANGED_SYMBOLS ||
    (detected.affected_processes?.length ?? 0) > MAX_AFFECTED_PROCESSES;
  const failedImpacts = impacts.filter(
    (impact) => impact.error !== undefined || impact.partial === true,
  ).length;
  const unresolvedImpacts = impacts.filter(
    (impact) =>
      impact.error === undefined &&
      impact.partial !== true &&
      normalizeRisk(impact.risk) === "unknown",
  ).length;
  const noteParts = [note];
  if (failedImpacts > 0) {
    noteParts.push(
      `${String(failedImpacts)} per-symbol caller lookup${failedImpacts === 1 ? "" : "s"} failed; the change-level result above is still complete.`,
    );
  }
  if (unresolvedImpacts > 0) {
    noteParts.push(
      `${String(unresolvedImpacts)} per-symbol caller lookup${unresolvedImpacts === 1 ? "" : "s"} could not resolve callers; confirm those symbols with a text search.`,
    );
  }

  return {
    ...context,
    risk,
    changedFiles: detected.summary?.changed_files ?? 0,
    changedSymbols: detected.summary?.changed_count ?? symbols.length,
    affectedProcesses: detected.summary?.affected_count ?? processes.length,
    truncated,
    partial,
    note: noteParts.filter((part): part is string => part !== null).join("\n\n") || null,
    symbols,
    processes,
    upstream: upstreamFrom(impacts),
  };
}

export interface ImpactTarget {
  readonly name: string;
  readonly path: string | undefined;
}

/// Targets worth asking GitNexus to trace upstream: the changed symbols,
/// bounded so a large pull request does not issue hundreds of queries. Methods
/// and properties dominate a changed-symbol list but produce mostly unresolved
/// caller lookups; functions, types and constants are what a reviewer needs.
/// The path disambiguates common names such as `new` or `build`.
export function impactTargets(detected: DetectChangesInput): readonly ImpactTarget[] {
  const preferred = (detected.changed_symbols ?? []).filter((symbol) =>
    IMPACT_KINDS.has(symbol.type ?? ""),
  );
  const seen = new Set<string>();
  const targets: ImpactTarget[] = [];
  for (const symbol of preferred) {
    if (typeof symbol.name !== "string" || symbol.name.length === 0) {
      continue;
    }
    const key = `${symbol.name}\0${symbol.filePath ?? ""}`;
    if (seen.has(key)) {
      continue;
    }
    seen.add(key);
    targets.push({ name: symbol.name, path: symbol.filePath });
    if (targets.length === MAX_IMPACT_TARGETS) {
      break;
    }
  }
  return targets;
}

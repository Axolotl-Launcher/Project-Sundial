// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

// Shared model and rendering for the pull-request impact report.
//
// The analysis job turns GitNexus output into this stable shape, and the
// comment job renders it from the uploaded artifact. Keeping the model here
// means the two jobs cannot drift, and the rendering has no dependency on the
// analysis environment.

export const REPORT_MARKER = "<!-- pr-impact-report -->";

export type RiskLevel =
  | "none"
  | "low"
  | "medium"
  | "high"
  | "critical"
  | "unknown";

export interface ChangedSymbol {
  readonly name: string;
  readonly type: string;
  readonly path: string;
}

export interface AffectedProcess {
  readonly name: string;
  readonly changedSteps: readonly string[];
  readonly stepCount: number;
}

export interface UpstreamCaller {
  readonly name: string;
  readonly risk: RiskLevel;
  readonly direct: number;
  readonly affectedProcesses: number;
}

export interface ImpactReport {
  readonly repository: string;
  readonly pullRequest: number;
  readonly headSha: string;
  readonly baseSha: string;
  readonly analyzedAt: string;
  readonly analyzer: string;
  readonly risk: RiskLevel;
  readonly changedFiles: number;
  readonly changedSymbols: number;
  readonly affectedProcesses: number;
  readonly truncated: boolean;
  readonly partial: boolean;
  readonly note: string | null;
  readonly symbols: readonly ChangedSymbol[];
  readonly processes: readonly AffectedProcess[];
  readonly upstream: readonly UpstreamCaller[];
}

const RISK_LABELS: Readonly<Record<RiskLevel, string>> = {
  none: "None",
  low: "Low",
  medium: "Medium",
  high: "High",
  critical: "Critical",
  unknown: "Unknown",
};

const RISK_ORDER: readonly RiskLevel[] = [
  "none",
  "low",
  "medium",
  "high",
  "critical",
  "unknown",
];

/// Highest risk present, treating `unknown` as the strongest caution.
export function highestRisk(risks: readonly RiskLevel[]): RiskLevel {
  let highest: RiskLevel = "none";
  for (const risk of risks) {
    if (RISK_ORDER.indexOf(risk) > RISK_ORDER.indexOf(highest)) {
      highest = risk;
    }
  }
  return highest;
}

export function riskLabel(risk: RiskLevel): string {
  return RISK_LABELS[risk];
}

/// Markdown table cells cannot contain raw pipes or newlines, and a symbol name
/// must not become HTML when GitHub renders the comment.
function cell(text: string): string {
  return text
    .replaceAll("&", "&amp;")
    .replaceAll("<", "&lt;")
    .replaceAll(">", "&gt;")
    .replaceAll("|", "\\|")
    .replaceAll("[", "\\[")
    .replaceAll("]", "\\]")
    .replaceAll("`", "\\`")
    .replaceAll(/\s*\r?\n\s*/g, " ");
}

function plural(count: number, singular: string, pluralForm: string): string {
  return `${String(count)} ${count === 1 ? singular : pluralForm}`;
}

/// A short, human-readable summary for the review comment.
export function summaryLine(report: ImpactReport): string {
  if (report.changedSymbols === 0) {
    return report.partial
      ? "No symbol-level impact was observed, but the analysis was incomplete."
      : "No indexed symbols were touched.";
  }
  return [
    plural(report.changedFiles, "file", "files"),
    plural(report.changedSymbols, "indexed symbol", "indexed symbols"),
    plural(report.affectedProcesses, "affected process", "affected processes"),
  ].join(", ");
}

/// The complete pull-request comment, including the marker used to update it.
export function renderComment(report: ImpactReport): string {
  const lines = [
    REPORT_MARKER,
    "## Impact analysis",
    "",
    `**Risk:** ${riskLabel(report.risk)}`,
    "",
    summaryLine(report),
    "",
  ];

  if (report.partial) {
    lines.push(
      "> [!WARNING]",
      "> GitNexus could not complete the analysis. The counts and risk below are lower bounds; do not read them as a clean result.",
      "",
    );
  }
  if (report.truncated) {
    lines.push(
      "> [!NOTE]",
      "> The changed-symbol list is capped. The counts and risk were computed over the full set.",
      "",
    );
  }
  if (report.risk === "unknown" && !report.partial) {
    lines.push(
      "> [!CAUTION]",
      "> The graph could not resolve the impact. Confirm with a text search before treating this change as safe.",
      "",
    );
  }
  if (report.note !== null) {
    lines.push(cell(report.note), "");
  }

  if (report.symbols.length > 0) {
    lines.push("### Changed symbols", "", "| Symbol | Kind | File |", "| --- | --- | --- |");
    for (const symbol of report.symbols) {
      lines.push(
        `| ${cell(symbol.name)} | ${cell(symbol.type)} | ${cell(symbol.path)} |`,
      );
    }
    lines.push("");
  }

  if (report.processes.length > 0) {
    lines.push("### Affected processes", "");
    for (const process of report.processes) {
      const steps =
        process.changedSteps.length === 0
          ? ""
          : ` — ${cell(process.changedSteps.join(", "))}`;
      lines.push(
        `- ${cell(process.name)} (${plural(process.stepCount, "step", "steps")})${steps}`,
      );
    }
    lines.push("");
  }

  if (report.upstream.length > 0) {
    lines.push(
      "### Upstream callers",
      "",
      "| Symbol | Risk | Direct callers | Affected processes |",
      "| --- | --- | --- | --- |",
    );
    for (const caller of report.upstream) {
      lines.push(
        `| ${cell(caller.name)} | ${riskLabel(caller.risk)} | ${String(caller.direct)} | ${String(caller.affectedProcesses)} |`,
      );
    }
    lines.push("");
  }

  lines.push(
    `Analyzed \`${report.baseSha.slice(0, 12)}\`..\`${report.headSha.slice(0, 12)}\` with ${report.analyzer}.`,
    "",
  );
  return lines.join("\n");
}

/// Mermaid mind-map source, kept in the artifact for later editing.
export function renderMermaid(report: ImpactReport): string {
  const lines = [
    "mindmap",
    `  root(("PR #${String(report.pullRequest)} · ${riskLabel(report.risk)}"))`,
    `    Changes["${report.changedFiles} files / ${report.changedSymbols} symbols"]`,
  ];
  const symbols = report.symbols.slice(0, 12);
  for (const symbol of symbols) {
    const label = `${symbol.name} (${symbol.type})`;
    lines.push(`      S${String(lines.length)}["${escapeMermaid(label)}"]`);
  }
  if (report.changedSymbols > symbols.length) {
    lines.push(
      `      More["+${String(report.changedSymbols - symbols.length)} more symbols"]`,
    );
  }
  if (report.processes.length > 0) {
    lines.push(`    Processes["${report.affectedProcesses} affected processes"]`);
    for (const process of report.processes.slice(0, 12)) {
      lines.push(`      P${String(lines.length)}["${escapeMermaid(process.name)}"]`);
    }
  }
  return `${lines.join("\n")}\n`;
}

function escapeMermaid(text: string): string {
  return text
    .replaceAll('"', "'")
    .replaceAll("[", "(")
    .replaceAll("]", ")")
    .replaceAll(/\s*\r?\n\s*/g, " ");
}

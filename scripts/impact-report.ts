// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

// The pull-request impact report model and its Markdown rendering.
//
// The analysis job writes this model and its rendering, and the comment job
// publishes the rendering. Keeping both here means the durable artifact and the
// posted comment cannot drift.

export const REPORT_MARKER = "<!-- pr-impact-report -->";

const SHA_PREFIX_LENGTH = 12;

export type ChangedFileKind = "source" | "test" | "other";

export interface ChangedFile {
  readonly path: string;
  readonly kind: ChangedFileKind;
}

/// Tests grouped by the crate directory they belong to, so the reading order
/// matches the module labels on the pull request.
export interface TestGroup {
  readonly module: string;
  readonly mirrored: boolean;
  readonly directory: string;
  readonly tests: readonly string[];
}

export interface IndexSummary {
  readonly fileCount: number;
  readonly nodeCount: number;
  readonly languages: readonly string[];
  readonly filesWithParseErrors: number;
}

export interface ImpactReport {
  readonly repository: string;
  readonly pullRequest: number;
  readonly headSha: string;
  readonly baseSha: string;
  readonly analyzedAt: string;
  readonly analyzer: string;
  readonly traversalDepth: number;
  readonly changedFileCount: number;
  readonly sourceFiles: number;
  readonly testFiles: number;
  readonly otherFiles: number;
  readonly changedFiles: readonly ChangedFile[];
  readonly changedFileListingLimit: number;
  readonly affectedTestCount: number;
  readonly testGroups: readonly TestGroup[];
  readonly testListingLimit: number;
  readonly touchedCycleCount: number;
  readonly cyclesByFile: Readonly<Record<string, number>>;
  readonly cycleListingLimit: number;
  readonly cycles: readonly (readonly string[])[];
  readonly truncatedCycles: boolean;
  readonly index: IndexSummary;
  readonly note: string | null;
}

/// Markdown table cells cannot contain raw pipes or newlines, and a path or
/// symbol must not become HTML when GitHub renders the comment.
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

/// The one-line summary under the title.
export function summaryLine(report: ImpactReport): string {
  if (report.changedFileCount === 0) {
    return "This pull request has no changed files against its merge base.";
  }
  const parts = [
    plural(report.changedFileCount, "changed file", "changed files"),
    `${String(report.sourceFiles)} source`,
    `${String(report.testFiles)} test`,
  ];
  if (report.otherFiles > 0) {
    parts.push(`${String(report.otherFiles)} other`);
  }
  parts.push(
    plural(report.affectedTestCount, "affected test file", "affected test files"),
  );
  return parts.join(" · ");
}

function appendWarnings(lines: string[]): void {
  lines.push(
    "> [!NOTE]",
    "> Advisory only: the affected test list is broad by design and does not gate this pull request. A missing index fails the job instead of posting an empty result.",
    "",
  );
}

function appendChangedFiles(lines: string[], report: ImpactReport): void {
  if (report.changedFiles.length === 0) {
    return;
  }
  lines.push("### Changed files", "", "| Kind | File |", "| --- | --- |");
  for (const file of report.changedFiles) {
    lines.push(`| ${file.kind} | ${cell(file.path)} |`);
  }
  const omitted = report.changedFileCount - report.changedFiles.length;
  if (omitted > 0) {
    lines.push(
      "",
      `> ${String(omitted)} further changed file${omitted === 1 ? "" : "s"} not listed.`,
    );
  }
  lines.push("");
}

function appendTestGroup(lines: string[], group: TestGroup, limit: number): void {
  const heading = group.mirrored ? "Mirrors" : "Dependent tests";
  lines.push(
    `#### ${heading} (\`${cell(group.directory)}\`)`,
    "",
  );
  if (!group.mirrored) {
    lines.push(
      `Reached through dependencies of the changed code rather than through a mirrored test file in this crate.`,
      "",
    );
  }
  for (const test of group.tests.slice(0, limit)) {
    lines.push(`- \`${cell(test)}\``);
  }
  const omitted = group.tests.length - Math.min(group.tests.length, limit);
  if (omitted > 0) {
    lines.push(`- … ${String(omitted)} more in this group`);
  }
  lines.push("");
}

function appendAffectedTests(lines: string[], report: ImpactReport): void {
  lines.push("### Affected tests", "");
  if (report.testGroups.length === 0) {
    lines.push(
      report.changedFileCount === 0
        ? "No changed file to map to tests."
        : "CodeGraph found no test file that depends on the changed sources. That can be correct, so confirm before treating the change as untested.",
      "",
    );
    return;
  }
  lines.push(
    `CodeGraph followed imports ${String(report.traversalDepth)} steps deep from the changed sources with \`codegraph affected\`.`,
    "",
  );
  for (const group of report.testGroups) {
    appendTestGroup(lines, group, report.testListingLimit);
  }
}

function appendCycles(lines: string[], report: ImpactReport): void {
  const cycleFileCount = Object.keys(report.cyclesByFile).length;
  lines.push("### Import cycles", "");
  if (report.touchedCycleCount === 0) {
    lines.push(
      "The changed files are not part of an import cycle the repository maintains. That covers the reported cycles only; it is not a proof the change is cycle-free.",
      "",
    );
    return;
  }
  lines.push(
    `The change touches ${plural(cycleFileCount, "import cycle file", "import cycle files")} (${String(report.touchedCycleCount)} touched ${report.touchedCycleCount === 1 ? "cycle" : "cycles"}):`,
    "",
  );
  for (const cycle of report.cycles) {
    const [head, ...rest] = cycle;
    if (head === undefined) {
      continue;
    }
    const cycleCount = report.cyclesByFile[head] ?? 0;
    const suffix = cycleCount > 1 ? ` (+${String(cycleCount - 1)} more)` : "";
    lines.push(`- \`${cell(head)}\`${suffix} → ${rest.map(cell).join(" → ")}`);
  }
  if (report.truncatedCycles) {
    lines.push(
      "",
      `> ${String(report.touchedCycleCount - report.cycleListingLimit)} further touched cycle${report.touchedCycleCount - report.cycleListingLimit === 1 ? "" : "s"} not listed.`,
    );
  }
  lines.push("");
}

/// The complete comment, including the marker used to update it in place.
export function renderComment(report: ImpactReport): string {
  const lines = [
    REPORT_MARKER,
    "## Impact analysis",
    "",
  ];
  appendWarnings(lines);
  lines.push(
    summaryLine(report),
    "",
    `\`codegraph status\`: ${plural(report.index.fileCount, "file", "files")} · ${plural(report.index.nodeCount, "node", "nodes")} · ${report.index.languages.length === 0 ? "no languages" : cell(report.index.languages.join(", "))}`,
    "",
  );
  appendChangedFiles(lines, report);
  appendAffectedTests(lines, report);
  appendCycles(lines, report);
  if (report.note !== null) {
    lines.push(
      ...report.note.split("\n\n").map((part) => `> ${cell(part)}`),
      "",
    );
  }
  lines.push(
    `Analyzed \`${report.baseSha.slice(0, SHA_PREFIX_LENGTH)}\`..\`${report.headSha.slice(0, SHA_PREFIX_LENGTH)}\` with ${report.analyzer}.`,
    "",
  );
  return lines.join("\n");
}

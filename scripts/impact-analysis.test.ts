// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

import assert from "node:assert/strict";
import { test } from "node:test";

import {
  buildReport,
  impactRisk,
  impactTargets,
  normalizeRisk,
} from "./impact-analysis.ts";

const SHA_LENGTH = 40;
const CHANGED_COUNT = 120;
const PROCESS_COUNT = 9;
const FILE_COUNT = 7;
const TWO_SYMBOLS = 2;
const MAX_IMPACT_TARGETS = 10;
const EXTRA_SYMBOLS = 5;

const context = {
  repository: "Axolotl-Launcher/Project-Sundial",
  pullRequest: 7,
  headSha: "a".repeat(SHA_LENGTH),
  baseSha: "b".repeat(SHA_LENGTH),
  analyzedAt: "2026-11-25T00:00:00.000Z",
  analyzer: "gitnexus@1.6.12",
} as const;

void test("risk names are case-insensitive", () => {
  assert.equal(normalizeRisk("HIGH"), "high");
  assert.equal(normalizeRisk(" unexpected "), "unknown");
});

void test("a partial detect result cannot be ranked", () => {
  const risk = impactRisk(
    { partial: true, summary: { risk_level: "low" } },
    [],
  );
  assert.equal(risk, "unknown");
});

void test("a higher symbol impact raises the change risk", () => {
  const risk = impactRisk(
    { summary: { risk_level: "low" } },
    [{ risk: "HIGH" }, { risk: "medium" }],
  );
  assert.equal(risk, "high");
});

void test("an unknown symbol lookup does not hide the change risk", () => {
  const risk = impactRisk(
    { summary: { risk_level: "critical" } },
    [
      { risk: "UNKNOWN", summary: { direct: 0 } },
      { risk: "low", summary: { direct: 1 } },
    ],
  );
  assert.equal(risk, "critical");
});

void test("a partial symbol impact cannot lower the risk", () => {
  const risk = impactRisk(
    { summary: { risk_level: "high" } },
    [{ partial: true, risk: "low" }],
  );
  assert.equal(risk, "high");
});

void test("report counts come from the complete summary", () => {
  const report = buildReport(
    context,
    {
      summary: {
        changed_count: CHANGED_COUNT,
        affected_count: PROCESS_COUNT,
        changed_files: FILE_COUNT,
        risk_level: "high",
      },
      changed_symbols: [{ name: "f", type: "Function", filePath: "a.rs" }],
      affected_processes: [
        { name: "p", step_count: 4, changed_steps: [{ symbol: "f" }, {}] },
      ],
      truncated: true,
    },
    [],
  );
  assert.equal(report.changedSymbols, CHANGED_COUNT);
  assert.equal(report.affectedProcesses, PROCESS_COUNT);
  assert.equal(report.changedFiles, FILE_COUNT);
  assert.equal(report.risk, "high");
  assert.equal(report.truncated, true);
  assert.deepEqual(report.processes[0]?.changedSteps, ["f"]);
  assert.deepEqual(report.upstream, []);
});

void test("a missing summary falls back to the listed symbols", () => {
  const report = buildReport(
    context,
    {
      changed_symbols: [
        { name: "f", type: "Function", filePath: "a.rs" },
        { name: "g", type: "Function", filePath: "b.rs" },
      ],
      affected_processes: [],
    },
    [],
  );
  assert.equal(report.changedSymbols, TWO_SYMBOLS);
  assert.equal(report.affectedProcesses, 0);
  assert.equal(report.risk, "unknown");
});

void test("analysis errors become a report note", () => {
  const report = buildReport(context, { error: "graph unavailable" }, []);
  assert.equal(report.note, "graph unavailable");
});

void test("successful caller lookups appear in the report", () => {
  const report = buildReport(
    context,
    { summary: { risk_level: "medium" } },
    [
      {
        target: { name: "prepare" },
        risk: "HIGH",
        summary: { direct: 3, processes_affected: 2 },
      },
    ],
  );
  assert.equal(report.risk, "high");
  assert.deepEqual(report.upstream, [
    { name: "prepare", risk: "high", direct: 3, affectedProcesses: 2 },
  ]);
});

void test("a failed caller lookup is disclosed but does not hide the change result", () => {
  const report = buildReport(
    context,
    { summary: { risk_level: "critical" } },
    [{ error: "ambiguous symbol" }],
  );
  assert.equal(report.risk, "critical");
  assert.match(report.note ?? "", /caller lookup failed/);
});

void test("impact targets are bounded and deduplicated by source order", () => {
  const detected = {
    changed_symbols: Array.from(
      { length: MAX_IMPACT_TARGETS + EXTRA_SYMBOLS },
      (_, index) => ({
        name: `symbol${String(index)}`,
        type: "Function",
        filePath: "a.rs",
      }),
    ),
  };
  assert.equal(impactTargets(detected).length, MAX_IMPACT_TARGETS);
  assert.deepEqual(
    impactTargets(detected).map((target) => target.name),
    [
      "symbol0",
      "symbol1",
      "symbol2",
      "symbol3",
      "symbol4",
      "symbol5",
      "symbol6",
      "symbol7",
      "symbol8",
      "symbol9",
    ],
  );
});

void test("impact targets keep the path for disambiguation", () => {
  const detected = {
    changed_symbols: [
      { name: "new", type: "Function", filePath: "a.rs" },
      { name: "new", type: "Function", filePath: "b.rs" },
    ],
  };
  assert.deepEqual(impactTargets(detected), [
    { name: "new", path: "a.rs" },
    { name: "new", path: "b.rs" },
  ]);
});

void test("properties do not crowd out functions in caller lookups", () => {
  const detected = {
    changed_symbols: [
      { name: "a", type: "Property", filePath: "a.rs" },
      { name: "b", type: "Property", filePath: "a.rs" },
      { name: "prepare", type: "Function", filePath: "a.rs" },
    ],
  };
  assert.deepEqual(impactTargets(detected), [
    { name: "prepare", path: "a.rs" },
  ]);
});

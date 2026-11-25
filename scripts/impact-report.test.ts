// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

// Tests for the impact report model and its renderers.

import assert from "node:assert/strict";
import { test } from "node:test";

import {
  highestRisk,
  renderComment,
  renderMermaid,
  REPORT_MARKER,
  type ImpactReport,
} from "./impact-report.ts";
import { renderDrawio, renderSvg } from "./impact-mindmap.ts";

const report: ImpactReport = {
  repository: "Axolotl-Launcher/Project-Sundial",
  pullRequest: 42,
  headSha: "0123456789abcdef0123456789abcdef01234567",
  baseSha: "89abcdef0123456789abcdef0123456789abcdef",
  analyzedAt: "2026-11-25T00:00:00.000Z",
  analyzer: "gitnexus@1.6.12",
  risk: "medium",
  changedFiles: 3,
  changedSymbols: 2,
  affectedProcesses: 1,
  callerLookupLimit: 10,
  callerLookupsOmitted: 0,
  truncated: false,
  partial: false,
  note: null,
  symbols: [
    { name: "prepare", type: "Function", path: "apps/core/install.rs" },
    { name: "LaunchPlan", type: "Struct", path: "apps/core/launch.rs" },
  ],
  processes: [
    { name: "launch_request", changedSteps: ["prepare", "launch"], stepCount: 4 },
  ],
  upstream: [
    { name: "prepare", risk: "low", direct: 1, affectedProcesses: 1 },
  ],
};

void test("report marker starts the comment", () => {
  assert.ok(renderComment(report).startsWith(REPORT_MARKER));
});

void test("comment records the risk and the analyzed range", () => {
  const comment = renderComment(report);
  assert.match(comment, /\*\*Risk:\*\* Medium/);
  assert.match(comment, /Analyzed/);
  assert.match(comment, /0123456789ab/);
});

void test("partial results warn instead of claiming a clean result", () => {
  const comment = renderComment({ ...report, partial: true });
  assert.match(comment, /\[!WARNING\]/);
  assert.match(comment, /lower bounds/);
});

void test("truncated results explain the capped list", () => {
  const comment = renderComment({ ...report, truncated: true });
  assert.match(comment, /\[!NOTE\]/);
  assert.match(comment, /capped/);
});

void test("upstream caller counts are visible", () => {
  const comment = renderComment(report);
  assert.match(comment, /### Upstream callers/);
  assert.match(comment, /\| prepare \| Low \| 1 \| 1 \|/);
});

void test("caller coverage limits are visible", () => {
  const comment = renderComment({
    ...report,
    callerLookupsOmitted: 3,
  });
  assert.match(comment, /3 further eligible symbols were not queried/);
});

void test("long process step lists are capped with a remainder", () => {
  const comment = renderComment({
    ...report,
    processes: [
      {
        name: "launch_request",
        changedSteps: Array.from({ length: 10 }, (_, index) => `step${String(index)}`),
        stepCount: 10,
      },
    ],
  });
  assert.match(comment, /step7/);
  assert.doesNotMatch(comment, /step8/);
  assert.match(comment, /\+2 more/);
});

void test("diagrams disclose hidden symbols and processes", () => {
  const large: ImpactReport = {
    ...report,
    symbols: Array.from({ length: 14 }, (_, index) => ({
      name: `symbol${String(index)}`,
      type: "Function",
      path: "a.rs",
    })),
    processes: Array.from({ length: 14 }, (_, index) => ({
      name: `process${String(index)}`,
      changedSteps: [],
      stepCount: 1,
    })),
  };
  assert.match(renderMermaid(large), /more symbols/);
  assert.match(renderMermaid(large), /more processes/);
  assert.match(renderDrawio(large), /more symbols/);
  assert.match(renderSvg(large), /more symbols/);
});

void test("unknown risk is not presented as safe", () => {
  const comment = renderComment({ ...report, risk: "unknown" });
  assert.match(comment, /\[!CAUTION\]/);
  assert.match(comment, /text search/);
});

void test("table cells escape pipes and newlines", () => {
  const comment = renderComment({
    ...report,
    symbols: [{ name: "use|field", type: "Function", path: "a\nb.rs" }],
  });
  assert.match(comment, /use\\\|field/);
  assert.match(comment, /a b\.rs/);
});

void test("the highest risk wins", () => {
  assert.equal(highestRisk(["low", "medium", "low"]), "medium");
  assert.equal(highestRisk(["critical", "unknown"]), "unknown");
  assert.equal(highestRisk([]), "none");
});

void test("an empty report still renders a valid comment and map", () => {
  const empty: ImpactReport = {
    ...report,
    risk: "none",
    changedFiles: 0,
    changedSymbols: 0,
    affectedProcesses: 0,
    symbols: [],
    processes: [],
    upstream: [],
  };
  assert.match(renderComment(empty), /No indexed symbols were touched/);
  assert.match(renderMermaid(empty), /mindmap/);
  assert.match(renderDrawio(empty), /<mxfile/);
  assert.match(renderSvg(empty), /<svg/);
});

void test("draw.io and SVG escape user-controlled labels", () => {
  const dangerous: ImpactReport = {
    ...report,
    symbols: [{ name: "<script>", type: "Function", path: "a&b.rs" }],
  };
  assert.doesNotMatch(renderDrawio(dangerous), /<script>/);
  assert.match(renderDrawio(dangerous), /&lt;script&gt;/);
  assert.doesNotMatch(renderSvg(dangerous), /<script>/);
  assert.match(renderSvg(dangerous), /&lt;script&gt;/);
});

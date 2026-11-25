// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

// draw.io and SVG rendering for the impact report.
//
// GitHub cannot render a `.drawio` file inline, so the workflow also uploads
// the Mermaid source and an SVG preview. The draw.io document keeps the same
// hierarchy for reviewers who want to edit it locally.

import {
  type ImpactReport,
  riskLabel,
} from "./impact-report.ts";

const WIDTH = 960;
const NODE_HEIGHT = 34;
const ROOT_WIDTH = 260;
const ROOT_X = 24;
const COLUMN_GAP = 60;
const ROW_GAP = 10;
const HALF = 2;
const BRANCH_DEPTH = 1;
const CHILD_DEPTH = 2;
const MAX_DIAGRAM_NODES = 12;
const MAX_SYMBOL_LABEL = 40;
const MAX_PROCESS_LABEL = 48;
const ROOT_Y = 24;
const ROOT_HEIGHT = 56;
const BRANCH_GAP = 30;
const EMPTY_LABEL_Y = 104;
const EMPTY_LABEL_HEIGHT = 24;
const MIN_PAGE_HEIGHT = 360;
const PAGE_PADDING = 140;
const PREVIEW_Y = 110;
const PREVIEW_ROW_HEIGHT = 40;
const PREVIEW_NODE_HEIGHT = 30;
const PREVIEW_LABEL_OFFSET = 8;
const MIN_PREVIEW_HEIGHT = 220;
const PREVIEW_BOTTOM_PADDING = 20;
const LABEL_LIMIT = 52;
const BRANCH_X = ROOT_Y + ROOT_WIDTH + COLUMN_GAP;
const BRANCH_WIDTH = 220;
const CHILD_X = BRANCH_X + BRANCH_WIDTH + COLUMN_GAP;
const CHILD_WIDTH = 300;
const PREVIEW_ROOT_X = 20;
const PREVIEW_ROOT_Y = 20;
const PREVIEW_ROOT_WIDTH = 240;
const PREVIEW_ROOT_HEIGHT = 52;
const PREVIEW_ROOT_RADIUS = 26;
const PREVIEW_ROOT_TEXT_X = 140;
const PREVIEW_ROOT_TEXT_Y = 51;
const PREVIEW_BRANCH_X = 300;
const PREVIEW_BRANCH_WIDTH = 220;
const PREVIEW_NODE_X = 560;
const PREVIEW_NODE_WIDTH = 360;

interface Node {
  readonly id: string;
  readonly label: string;
  readonly depth: number;
}

function escapeXml(text: string): string {
  return text
    .replaceAll("&", "&amp;")
    .replaceAll("<", "&lt;")
    .replaceAll(">", "&gt;")
    .replaceAll('"', "&quot;")
    .replaceAll("'", "&apos;");
}

function shortText(text: string, limit: number): string {
  if (text.length <= limit) {
    return text;
  }
  return `${text.slice(0, limit - 1)}…`;
}

function nodesFor(report: ImpactReport, limit: number | undefined): Node[] {
  const nodes: Node[] = [];
  const symbols = limit === undefined ? report.symbols : report.symbols.slice(0, limit);
  const processes = limit === undefined ? report.processes : report.processes.slice(0, limit);
  if (report.symbols.length > 0) {
    nodes.push({ id: "changes", label: "Changed symbols", depth: 1 });
    for (const [index, symbol] of symbols.entries()) {
      nodes.push({
        id: `symbol-${String(index)}`,
        label: `${shortText(symbol.name, MAX_SYMBOL_LABEL)} · ${symbol.type}`,
        depth: 2,
      });
    }
    const omitted = report.symbols.length - symbols.length;
    if (omitted > 0) {
      nodes.push({
        id: "symbol-more",
        label: `+${String(omitted)} more symbols`,
        depth: 2,
      });
    }
  }
  if (report.processes.length > 0) {
    nodes.push({ id: "processes", label: "Affected processes", depth: 1 });
    for (const [index, process] of processes.entries()) {
      nodes.push({
        id: `process-${String(index)}`,
        label: shortText(process.name, MAX_PROCESS_LABEL),
        depth: 2,
      });
    }
    const omitted = report.processes.length - processes.length;
    if (omitted > 0) {
      nodes.push({
        id: "process-more",
        label: `+${String(omitted)} more processes`,
        depth: 2,
      });
    }
  }
  return nodes;
}

/// Vertically stacked mind-map cells. draw.io accepts fixed geometry, which
/// keeps the layout deterministic and avoids pulling a layout library into CI.
function cells(report: ImpactReport, idPrefix: string): string {
  const nodes = nodesFor(report, MAX_DIAGRAM_NODES);
  const root: Node = {
    id: "root",
    label: `PR #${String(report.pullRequest)} · ${riskLabel(report.risk)}`,
    depth: 0,
  };
  const branches = nodes.filter((node) => node.depth === BRANCH_DEPTH);
  const children = nodes.filter((node) => node.depth === CHILD_DEPTH);
  const output: string[] = [
    `<mxCell id="${idPrefix}-root" value="${escapeXml(root.label)}" style="ellipse;whiteSpace=wrap;html=0;fillColor=#dae8fc;strokeColor=#6c8ebf;fontStyle=1;" vertex="1" parent="1"><mxGeometry x="${String(ROOT_X)}" y="${String(ROOT_Y)}" width="${String(ROOT_WIDTH)}" height="${String(ROOT_HEIGHT)}" as="geometry"/></mxCell>`,
  ];

  let y = ROOT_Y;
  let childIndex = 0;
  let branchY = ROOT_Y;
  for (const branch of branches) {
    const branchLimit =
      branch.id === "changes"
        ? Math.min(report.symbols.length, MAX_DIAGRAM_NODES) +
          (report.symbols.length > MAX_DIAGRAM_NODES ? 1 : 0)
        : Math.min(report.processes.length, MAX_DIAGRAM_NODES) +
          (report.processes.length > MAX_DIAGRAM_NODES ? 1 : 0);
    const branchChildren = children.slice(
      childIndex,
      childIndex + branchLimit,
    );
    childIndex += branchChildren.length;
    const branchHeight = Math.max(NODE_HEIGHT, branchChildren.length * (NODE_HEIGHT + ROW_GAP) - ROW_GAP);
    output.push(
      `<mxCell id="${idPrefix}-${branch.id}" value="${escapeXml(branch.label)}" style="rounded=1;whiteSpace=wrap;html=0;fillColor=#fff2cc;strokeColor=#d6b656;fontStyle=1;" vertex="1" parent="1"><mxGeometry x="${String(BRANCH_X)}" y="${String(branchY)}" width="${String(BRANCH_WIDTH)}" height="${String(NODE_HEIGHT)}" as="geometry"/></mxCell>`,
    );
    output.push(
      `<mxCell id="${idPrefix}-edge-${branch.id}" style="edgeStyle=orthogonalEdgeStyle;rounded=0;html=0;endArrow=none;" edge="1" parent="1" source="${idPrefix}-root" target="${idPrefix}-${branch.id}"><mxGeometry relative="1" as="geometry"/></mxCell>`,
    );
    let childY = branchY;
    for (const child of branchChildren) {
      output.push(
        `<mxCell id="${idPrefix}-${child.id}" value="${escapeXml(child.label)}" style="rounded=0;whiteSpace=wrap;html=0;fillColor=#f5f5f5;strokeColor=#666666;" vertex="1" parent="1"><mxGeometry x="${String(CHILD_X)}" y="${String(childY)}" width="${String(CHILD_WIDTH)}" height="${String(NODE_HEIGHT)}" as="geometry"/></mxCell>`,
      );
      output.push(
        `<mxCell id="${idPrefix}-edge-${child.id}" style="edgeStyle=orthogonalEdgeStyle;rounded=0;html=0;endArrow=none;" edge="1" parent="1" source="${idPrefix}-${branch.id}" target="${idPrefix}-${child.id}"><mxGeometry relative="1" as="geometry"/></mxCell>`,
      );
      childY += NODE_HEIGHT + ROW_GAP;
    }
    y = Math.max(y, branchY + branchHeight);
    branchY = y + BRANCH_GAP;
  }

  if (branches.length === 0) {
    output.push(
    `<mxCell id="${idPrefix}-empty" value="${escapeXml(`PR #${String(report.pullRequest)}`)}" style="text;html=0;align=left;verticalAlign=middle;fontSize=12;" vertex="1" parent="1"><mxGeometry x="${String(ROOT_X)}" y="${String(EMPTY_LABEL_Y)}" width="${String(ROOT_WIDTH * HALF)}" height="${String(EMPTY_LABEL_HEIGHT)}" as="geometry"/></mxCell>`,
    );
  }
  return output.join("");
}

/// A draw.io XML document for the impact report.
export function renderDrawio(report: ImpactReport): string {
  const height = Math.max(
    MIN_PAGE_HEIGHT,
    PAGE_PADDING +
      (Math.min(report.symbols.length, MAX_DIAGRAM_NODES) +
        Math.min(report.processes.length, MAX_DIAGRAM_NODES) +
        (report.symbols.length > MAX_DIAGRAM_NODES ? 1 : 0) +
        (report.processes.length > MAX_DIAGRAM_NODES ? 1 : 0)) *
        (NODE_HEIGHT + ROW_GAP),
  );
  return [
    `<mxfile host="axolotl-ci" modified="${escapeXml(report.analyzedAt)}" agent="impact-report" version="24.7.17">`,
    `<diagram id="impact" name="PR #${String(report.pullRequest)} impact">`,
    `<mxGraphModel dx="${String(WIDTH)}" dy="${String(height)}" grid="1" gridSize="10" guides="1" tooltips="1" connect="1" arrows="1" fold="1" page="1" pageScale="1" pageWidth="${String(WIDTH)}" pageHeight="${String(height)}" math="0" shadow="0">`,
    "<root>",
    '<mxCell id="0"/>',
    '<mxCell id="1" parent="0"/>',
    cells(report, "pr-impact"),
    "</root>",
    "</mxGraphModel>",
    "</diagram>",
    "</mxfile>",
  ].join("");
}

/// A standalone SVG preview that can be opened without draw.io.
export function renderSvg(report: ImpactReport): string {
  const rows: string[] = [];
  const rootLabel = `PR #${String(report.pullRequest)} · ${riskLabel(report.risk)}`;
  rows.push(
    `<rect x="${String(PREVIEW_ROOT_X)}" y="${String(PREVIEW_ROOT_Y)}" width="${String(PREVIEW_ROOT_WIDTH)}" height="${String(PREVIEW_ROOT_HEIGHT)}" rx="${String(PREVIEW_ROOT_RADIUS)}" fill="#dae8fc" stroke="#6c8ebf"/>`,
    `<text x="${String(PREVIEW_ROOT_TEXT_X)}" y="${String(PREVIEW_ROOT_TEXT_Y)}" text-anchor="middle" font-family="sans-serif" font-size="16" font-weight="600">${escapeXml(rootLabel)}</text>`,
  );
  let y = PREVIEW_Y;
  const addRow = (label: string, x: number, width: number, fill: string): void => {
    rows.push(
      `<rect x="${String(x)}" y="${String(y)}" width="${String(width)}" height="${String(PREVIEW_NODE_HEIGHT)}" rx="4" fill="${fill}" stroke="#888888"/>`,
      `<text x="${String(x + PREVIEW_LABEL_OFFSET)}" y="${String(y + PREVIEW_LABEL_OFFSET + PREVIEW_NODE_HEIGHT / HALF)}" font-family="sans-serif" font-size="12">${escapeXml(shortText(label, LABEL_LIMIT))}</text>`,
    );
    y += PREVIEW_ROW_HEIGHT;
  };
  if (report.symbols.length > 0) {
    addRow("Changed symbols", PREVIEW_BRANCH_X, PREVIEW_BRANCH_WIDTH, "#fff2cc");
    for (const symbol of nodesFor(report, MAX_DIAGRAM_NODES).filter((node) => node.id.startsWith("symbol"))) {
      addRow(symbol.label, PREVIEW_NODE_X, PREVIEW_NODE_WIDTH, "#f5f5f5");
    }
  }
  if (report.processes.length > 0) {
    addRow("Affected processes", PREVIEW_BRANCH_X, PREVIEW_BRANCH_WIDTH, "#fff2cc");
    for (const process of nodesFor(report, MAX_DIAGRAM_NODES).filter((node) => node.id.startsWith("process"))) {
      addRow(process.label, PREVIEW_NODE_X, PREVIEW_NODE_WIDTH, "#f5f5f5");
    }
  }
  const height = Math.max(MIN_PREVIEW_HEIGHT, y + PREVIEW_BOTTOM_PADDING);
  return [
    `<svg xmlns="http://www.w3.org/2000/svg" width="${String(WIDTH)}" height="${String(height)}" viewBox="0 0 ${String(WIDTH)} ${String(height)}">`,
    `<rect width="${String(WIDTH)}" height="${String(height)}" fill="#ffffff"/>`,
    ...rows,
    "</svg>",
  ].join("");
}

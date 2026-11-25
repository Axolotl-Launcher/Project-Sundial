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
const COLUMN_GAP = 60;
const ROW_GAP = 10;

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

function nodesFor(report: ImpactReport): Node[] {
  const nodes: Node[] = [];
  if (report.symbols.length > 0) {
    nodes.push({ id: "changes", label: "Changed symbols", depth: 1 });
    for (const [index, symbol] of report.symbols.slice(0, 12).entries()) {
      nodes.push({
        id: `symbol-${String(index)}`,
        label: `${shortText(symbol.name, 40)} · ${symbol.type}`,
        depth: 2,
      });
    }
  }
  if (report.processes.length > 0) {
    nodes.push({ id: "processes", label: "Affected processes", depth: 1 });
    for (const [index, process] of report.processes.slice(0, 12).entries()) {
      nodes.push({
        id: `process-${String(index)}`,
        label: shortText(process.name, 48),
        depth: 2,
      });
    }
  }
  return nodes;
}

/// Vertically stacked mind-map cells. draw.io accepts fixed geometry, which
/// keeps the layout deterministic and avoids pulling a layout library into CI.
function cells(report: ImpactReport, idPrefix: string): string {
  const nodes = nodesFor(report);
  const root: Node = {
    id: "root",
    label: `PR #${String(report.pullRequest)} · ${riskLabel(report.risk)}`,
    depth: 0,
  };
  const branches = nodes.filter((node) => node.depth === 1);
  const children = nodes.filter((node) => node.depth === 2);
  const output: string[] = [
    `<mxCell id="${idPrefix}-root" value="${escapeXml(root.label)}" style="ellipse;whiteSpace=wrap;html=0;fillColor=#dae8fc;strokeColor=#6c8ebf;fontStyle=1;" vertex="1" parent="1"><mxGeometry x="24" y="24" width="${String(ROOT_WIDTH)}" height="56" as="geometry"/></mxCell>`,
  ];

  let y = 24;
  let childIndex = 0;
  let branchY = 24;
  for (const branch of branches) {
    const branchChildren = children.slice(
      childIndex,
      childIndex + (branch.id === "changes" ? Math.min(12, report.symbols.length) : Math.min(12, report.processes.length)),
    );
    childIndex += branchChildren.length;
    const branchHeight = Math.max(NODE_HEIGHT, branchChildren.length * (NODE_HEIGHT + ROW_GAP) - ROW_GAP);
    output.push(
      `<mxCell id="${idPrefix}-${branch.id}" value="${escapeXml(branch.label)}" style="rounded=1;whiteSpace=wrap;html=0;fillColor=#fff2cc;strokeColor=#d6b656;fontStyle=1;" vertex="1" parent="1"><mxGeometry x="${String(24 + ROOT_WIDTH + COLUMN_GAP)}" y="${String(branchY)}" width="220" height="${String(NODE_HEIGHT)}" as="geometry"/></mxCell>`,
    );
    output.push(
      `<mxCell id="${idPrefix}-edge-${branch.id}" style="edgeStyle=orthogonalEdgeStyle;rounded=0;html=0;endArrow=none;" edge="1" parent="1" source="${idPrefix}-root" target="${idPrefix}-${branch.id}"><mxGeometry relative="1" as="geometry"/></mxCell>`,
    );
    let childY = branchY;
    for (const child of branchChildren) {
      output.push(
        `<mxCell id="${idPrefix}-${child.id}" value="${escapeXml(child.label)}" style="rounded=0;whiteSpace=wrap;html=0;fillColor=#f5f5f5;strokeColor=#666666;" vertex="1" parent="1"><mxGeometry x="${String(24 + ROOT_WIDTH + COLUMN_GAP + 220 + COLUMN_GAP)}" y="${String(childY)}" width="300" height="${String(NODE_HEIGHT)}" as="geometry"/></mxCell>`,
      );
      output.push(
        `<mxCell id="${idPrefix}-edge-${child.id}" style="edgeStyle=orthogonalEdgeStyle;rounded=0;html=0;endArrow=none;" edge="1" parent="1" source="${idPrefix}-${branch.id}" target="${idPrefix}-${child.id}"><mxGeometry relative="1" as="geometry"/></mxCell>`,
      );
      childY += NODE_HEIGHT + ROW_GAP;
    }
    y = Math.max(y, branchY + branchHeight);
    branchY = y + 30;
  }

  if (branches.length === 0) {
    output.push(
      `<mxCell id="${idPrefix}-empty" value="${escapeXml(`PR #${String(report.pullRequest)}`)}" style="text;html=0;align=left;verticalAlign=middle;fontSize=12;" vertex="1" parent="1"><mxGeometry x="24" y="104" width="520" height="24" as="geometry"/></mxCell>`,
    );
  }
  return output.join("");
}

/// A draw.io XML document for the impact report.
export function renderDrawio(report: ImpactReport): string {
  const height = Math.max(
    360,
    140 +
      (Math.min(report.symbols.length, 12) + Math.min(report.processes.length, 12)) *
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
    `<rect x="20" y="20" width="240" height="52" rx="26" fill="#dae8fc" stroke="#6c8ebf"/>`,
    `<text x="140" y="51" text-anchor="middle" font-family="sans-serif" font-size="16" font-weight="600">${escapeXml(rootLabel)}</text>`,
  );
  let y = 110;
  const addRow = (label: string, x: number, width: number, fill: string): void => {
    rows.push(
      `<rect x="${String(x)}" y="${String(y)}" width="${String(width)}" height="30" rx="4" fill="${fill}" stroke="#888888"/>`,
      `<text x="${String(x + 8)}" y="${String(y + 20)}" font-family="sans-serif" font-size="12">${escapeXml(shortText(label, 52))}</text>`,
    );
    y += 40;
  };
  if (report.symbols.length > 0) {
    addRow("Changed symbols", 300, 220, "#fff2cc");
    for (const symbol of report.symbols.slice(0, 12)) {
      addRow(`${symbol.name} · ${symbol.type}`, 560, 360, "#f5f5f5");
    }
  }
  if (report.processes.length > 0) {
    addRow("Affected processes", 300, 220, "#fff2cc");
    for (const process of report.processes.slice(0, 12)) {
      addRow(process.name, 560, 360, "#f5f5f5");
    }
  }
  const height = Math.max(220, y + 20);
  return [
    `<svg xmlns="http://www.w3.org/2000/svg" width="960" height="${String(height)}" viewBox="0 0 960 ${String(height)}">`,
    `<rect width="960" height="${String(height)}" fill="#ffffff"/>`,
    ...rows,
    "</svg>",
  ].join("");
}

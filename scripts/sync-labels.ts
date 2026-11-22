// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

// Applies the label vocabulary to this repository.
//
// Issue forms can only carry the static labels in their front matter, and pull
// requests cannot label themselves at all, so the vocabulary is declared here
// and applied by `triage.yml`. Run this after changing LABELS: it creates what
// is missing and updates colours and descriptions in place.

import { execFileSync } from "node:child_process";

interface LabelSpec {
  readonly name: string;
  readonly color: string;
  readonly description: string;
}

const TYPE_LABELS: readonly LabelSpec[] = [
  { name: "type: bug", color: "d73a4a", description: "Something behaves incorrectly" },
  { name: "type: feature", color: "a2eeef", description: "A capability or behaviour change" },
  { name: "type: docs", color: "0075ca", description: "Documentation only" },
  { name: "type: chore", color: "ededed", description: "Maintenance, no behaviour change" },
];

const PLATFORM_LABELS: readonly LabelSpec[] = [
  { name: "platform: windows", color: "0078d4", description: "Windows desktop shell" },
  { name: "platform: linux", color: "f9a825", description: "Linux desktop shell" },
  { name: "platform: macos", color: "555555", description: "macOS desktop shell" },
  { name: "platform: all", color: "5319e7", description: "Reproduces on every platform" },
];

const MODULE_SLUGS: readonly [string, string][] = [
  ["domain", "Domain identifiers and base types"],
  ["content", "Content model and providers"],
  ["install", "Installation planning and execution"],
  ["launch", "Launching, Java discovery, process lifecycle"],
  ["state", "Application state and persistence"],
  ["storage", "Filesystem and cache primitives"],
  ["network", "HTTP and protocol clients"],
  ["telemetry", "Event sink and spans"],
  ["scheduler", "Background work scheduling"],
  ["resources", "Favourites, history, fingerprints"],
  ["mod-metadata", "Loaders and mod metadata"],
  ["observability", "Command metrics and diagnostics"],
  ["ui", "Palette and gpui-kit vocabulary"],
  ["frontend", "State, layout and window wiring"],
  ["platforms", "Platform shells"],
  ["repository", "Build, tooling and CI"],
  ["unknown", "Module not yet determined"],
];

const MODULE_LABELS: readonly LabelSpec[] = MODULE_SLUGS.map(([slug, description]) => ({
  name: `module: ${slug}`,
  color: "0e8a16",
  description,
}));

const STATUS_LABELS: readonly LabelSpec[] = [
  { name: "status: unreviewed", color: "fbca04", description: "No maintainer has reviewed this yet" },
  { name: "status: reviewed", color: "0e8a16", description: "A maintainer has reviewed this" },
];

export const LABELS: readonly LabelSpec[] = [
  ...TYPE_LABELS,
  ...PLATFORM_LABELS,
  ...MODULE_LABELS,
  ...STATUS_LABELS,
];

/// Colour used for `version: *` labels created on demand.
const VERSION_COLOR = "bfd4f2";

function upsert(label: LabelSpec): void {
  execFileSync(
    "gh",
    [
      "label",
      "create",
      label.name,
      "--color",
      label.color,
      "--description",
      label.description,
      "--force",
    ],
    { stdio: "inherit" },
  );
}

function main(): number {
  for (const label of LABELS) {
    upsert(label);
  }
  console.log(`applied ${String(LABELS.length)} labels`);
  console.log(`version labels use colour ${VERSION_COLOR}`);
  return 0;
}

process.exit(main());

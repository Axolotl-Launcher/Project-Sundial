// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

// Applies `platform:`, `module:` and `version:` labels to an opened issue.
//
// The body arrives from `triage.yml` as ISSUE_BODY. Labels the form's front
// matter already applied (the `type:` label) are left alone.

import { execFileSync } from "node:child_process";

import { moduleLabels, platformLabels, versionLabels } from "./issue-labels.ts";
import { ensureVersionLabel } from "./version-labels.ts";

/// Issue forms render an unanswered optional field this way.
const NO_RESPONSE = "_No response_";

function apply(number: string, labels: readonly string[]): void {
  if (labels.length === 0) {
    return;
  }
  execFileSync(
    "gh",
    ["issue", "edit", number, ...labels.flatMap((label) => ["--add-label", label])],
    { stdio: "inherit" },
  );
}

function main(): number {
  const number = process.env["ISSUE_NUMBER"];
  const body = process.env["ISSUE_BODY"] ?? "";

  if (number === undefined) {
    console.error("ISSUE_NUMBER is not set");
    return 1;
  }
  if (body === "" || body === NO_RESPONSE) {
    return 0;
  }

  const versions = versionLabels(body);
  for (const label of versions) {
    ensureVersionLabel(label);
  }

  apply(number, [...platformLabels(body), ...moduleLabels(body), ...versions]);
  return 0;
}

process.exit(main());

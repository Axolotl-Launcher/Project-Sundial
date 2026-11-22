// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

// Creates `version:` labels on demand.
//
// The static vocabulary in `sync-labels.ts` cannot list them: every release
// adds one, and an issue can name a version that does not exist yet. A missing
// label makes `gh ... --add-label` reject the whole command, so the triage
// scripts create the label before applying it.

import { execFileSync } from "node:child_process";

/// Colour shared by every `version: *` label.
export const VERSION_COLOR = "bfd4f2";

/// Creates a version label, or updates one an earlier run created.
export function ensureVersionLabel(label: string): void {
  execFileSync(
    "gh",
    [
      "label",
      "create",
      label,
      "--color",
      VERSION_COLOR,
      "--description",
      "Version a report or pull request belongs to",
      "--force",
    ],
    { stdio: "inherit" },
  );
}

// SPDX-FileCopyrightText: 2026 The Superlogical Authors
// SPDX-License-Identifier: GPL-3.0-only

// Maps changed paths to `module:` labels.
//
// A pull request cannot describe itself in a machine-readable way: its template
// is markdown, not a form. The paths it touches are the reliable signal, so
// this table mirrors the crate layout.
//
// Paths are matched in order and every match is applied, so a change under
// apps/core/ui also selects the frontend when it touches both.

export interface ModuleRoute {
  readonly prefix: string;
  readonly module: string;
}

export const MODULE_ROUTES: readonly ModuleRoute[] = [
  { prefix: "apps/core/domain/", module: "domain" },
  { prefix: "apps/core/content/", module: "content" },
  { prefix: "apps/core/install/", module: "install" },
  { prefix: "apps/core/launch/", module: "launch" },
  { prefix: "apps/core/state/", module: "state" },
  { prefix: "apps/core/storage/", module: "storage" },
  { prefix: "apps/core/network/", module: "network" },
  { prefix: "apps/core/telemetry/", module: "telemetry" },
  { prefix: "apps/core/scheduler/", module: "scheduler" },
  { prefix: "apps/core/resources/", module: "resources" },
  { prefix: "apps/core/mod_metadata/", module: "mod-metadata" },
  { prefix: "apps/core/observability/", module: "observability" },
  { prefix: "apps/core/spi/", module: "domain" },
  { prefix: "apps/core/", module: "domain" },
  { prefix: "apps/ui/", module: "ui" },
  { prefix: "apps/frontend/", module: "frontend" },
  { prefix: "platforms/", module: "platforms" },
  { prefix: "tests/", module: "repository" },
  { prefix: "scripts/", module: "repository" },
  { prefix: ".github/", module: "repository" },
  { prefix: "packaging/", module: "repository" },
];

/// Repository files that are not owned by a module.
const REPOSITORY_ROOT_FILES: readonly string[] = [
  "Cargo.toml",
  "Cargo.lock",
  "package.json",
  "pnpm-lock.yaml",
  "pnpm-workspace.yaml",
  "turbo.jsonc",
  "tsconfig.json",
  "prek.toml",
  "deny.toml",
  "clippy.toml",
  "rust-toolchain.toml",
  "rustfmt.toml",
  "tombi.toml",
  "_typos.toml",
];

/// Modules touched by a set of changed paths, deduplicated and sorted.
export function modulesForPaths(paths: readonly string[]): string[] {
  const found = new Set<string>();

  for (const path of paths) {
    if (REPOSITORY_ROOT_FILES.includes(path)) {
      found.add("repository");
      continue;
    }
    for (const route of MODULE_ROUTES) {
      if (path.startsWith(route.prefix)) {
        found.add(route.module);
        break;
      }
    }
  }

  return [...found].sort();
}

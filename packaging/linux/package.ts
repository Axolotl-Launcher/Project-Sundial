// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

// Steps the packaging scripts share: where the tree is, which version it
// carries, and how a file lands in a package root.

import { chmodSync, copyFileSync, mkdirSync, readFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";

export const repoRoot = resolve(import.meta.dirname, "..", "..");

/// File modes the package payload uses.
export const EXECUTABLE = 0o755;
export const READABLE = 0o644;

/// The workspace manifest is the only place the version is written down.
export function workspaceVersion(): string {
  const manifest = readFileSync(join(repoRoot, "Cargo.toml"), "utf8");
  const version = /^version = "(.*)"$/m.exec(manifest)?.[1];
  if (version === undefined) {
    throw new Error("Cargo.toml declares no workspace version");
  }
  return version;
}

export function copyInto(source: string, target: string, mode: number): void {
  mkdirSync(dirname(target), { recursive: true });
  copyFileSync(source, target);
  chmodSync(target, mode);
}

/// `node script.ts <first> <second>`: neither argument has a default.
export function argument(value: string | undefined, name: string): string {
  if (value === undefined) {
    throw new Error(`missing argument: ${name}`);
  }
  return value;
}

export function requireTrue(condition: boolean, message: string): void {
  if (!condition) {
    throw new Error(message);
  }
}

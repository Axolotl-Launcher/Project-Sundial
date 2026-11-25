// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

// Reads and validates the impact artifact published by the analysis job.

import { readFileSync, statSync } from "node:fs";

const MAX_ARTIFACT_BYTES = 5 * 1024 * 1024;

export function readArtifact(path: string): unknown {
  const size = statSync(path).size;
  if (size > MAX_ARTIFACT_BYTES) {
    throw new Error(`impact artifact exceeds ${String(MAX_ARTIFACT_BYTES)} bytes`);
  }
  return JSON.parse(readFileSync(path, "utf8")) as unknown;
}

export function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null;
}

export function assertString(value: unknown, field: string): string {
  if (typeof value !== "string") {
    throw new Error(`impact artifact has no string "${field}"`);
  }
  return value;
}

export function assertNumber(value: unknown, field: string): number {
  if (typeof value !== "number" || !Number.isFinite(value)) {
    throw new Error(`impact artifact has no number "${field}"`);
  }
  return value;
}

export function assertInteger(value: unknown, field: string): number {
  const number = assertNumber(value, field);
  if (!Number.isInteger(number) || number <= 0) {
    throw new Error(`impact artifact has an invalid "${field}"`);
  }
  return number;
}

export function assertBoolean(value: unknown, field: string): boolean {
  if (typeof value !== "boolean") {
    throw new Error(`impact artifact has no boolean "${field}"`);
  }
  return value;
}

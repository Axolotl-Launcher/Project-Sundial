// SPDX-FileCopyrightText: 2026 The Superlogical Authors
// SPDX-License-Identifier: GPL-3.0-only

// Turns an issue form's rendered body into labels.
//
// GitHub renders a form answer as markdown headings plus the value:
//
//     ### Platform
//
//     Windows
//
// Issue forms can only apply the static `labels:` list from their front matter,
// so the choice a reporter makes in a dropdown cannot become a label on its
// own. This module maps those answers to the label vocabulary the workflows
// apply.

/// Label prefixes. Keeping them consistent lets a filter like
/// `label:"module: *"` select every module.
export const PREFIX = {
  platform: "platform: ",
  module: "module: ",
  version: "version: ",
  type: "type: ",
  status: "status: ",
} as const;

/// Values a platform dropdown may offer.
const PLATFORM_VALUES: ReadonlyMap<string, string> = new Map([
  ["windows", "windows"],
  ["linux", "linux"],
  ["macos", "macos"],
  ["all platforms", "all"],
]);

/// Values a module dropdown may offer.
const MODULE_VALUES: ReadonlyMap<string, string> = new Map([
  ["domain", "domain"],
  ["content", "content"],
  ["install", "install"],
  ["launch", "launch"],
  ["state", "state"],
  ["storage", "storage"],
  ["network", "network"],
  ["telemetry", "telemetry"],
  ["scheduler", "scheduler"],
  ["resources", "resources"],
  ["mod metadata", "mod-metadata"],
  ["observability", "observability"],
  ["ui", "ui"],
  ["frontend", "frontend"],
  ["platform shells", "platforms"],
  ["build and tooling", "repository"],
  ["not sure", "unknown"],
]);

/// A rendered form section: its heading text and the value beneath it.
export interface Section {
  readonly heading: string;
  readonly value: string;
}

/// Splits a rendered issue body into `### heading` sections.
export function sections(body: string): Section[] {
  const lines = body.split("\n");
  const result: Section[] = [];
  let heading: string | null = null;
  let buffer: string[] = [];

  const flush = (): void => {
    if (heading !== null) {
      result.push({ heading, value: buffer.join("\n").trim() });
    }
    buffer = [];
  };

  for (const line of lines) {
    const match = /^###\s+(.+?)\s*$/.exec(line);
    if (match !== null) {
      flush();
      heading = match[1] ?? "";
      continue;
    }
    if (heading !== null) {
      buffer.push(line);
    }
  }
  flush();
  return result;
}

/// Finds the value of the section whose heading matches `name`, ignoring case
/// and surrounding whitespace.
export function answer(body: string, name: string): string | undefined {
  const wanted = name.trim().toLowerCase();
  const found = sections(body).find(
    (section) => section.heading.trim().toLowerCase() === wanted,
  );
  const value = found?.value.trim();
  return value === undefined || value === "" ? undefined : value;
}

/// Maps a dropdown answer onto a label, or `undefined` when the answer is not
/// one the dropdown offers.
export function mapValue(
  table: ReadonlyMap<string, string>,
  value: string | undefined,
  prefix: string,
): string | undefined {
  if (value === undefined) {
    return undefined;
  }
  const key = value.trim().toLowerCase();
  const slug = table.get(key);
  return slug === undefined ? undefined : prefix + slug;
}

/// The heading each form uses, so the maps below stay in one place.
export const HEADING = {
  platform: "Platforms affected",
  module: "Affected module",
  version: "Version",
} as const;

/// Labels for the platform a report names.
export function platformLabels(body: string): string[] {
  const label = mapValue(PLATFORM_VALUES, answer(body, HEADING.platform), PREFIX.platform);
  return label === undefined ? [] : [label];
}

/// Labels for the module a report names.
export function moduleLabels(body: string): string[] {
  const raw = answer(body, HEADING.module);
  const label = mapValue(MODULE_VALUES, raw, PREFIX.module);
  return label === undefined ? [] : [label];
}

/// A version label, normalised to the shape the release tags use.
///
/// The answer is free text, so anything that is not a version number is
/// dropped rather than becoming a label nobody can filter on.
export function versionLabels(body: string): string[] {
  const raw = answer(body, HEADING.version);
  if (raw === undefined) {
    return [];
  }
  const match = /^v?(\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?)$/.exec(raw.trim());
  return match?.[1] === undefined ? [] : [PREFIX.version + match[1]];
}

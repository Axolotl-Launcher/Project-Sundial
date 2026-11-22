// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

// Tests for the issue-form label mapping.

import assert from "node:assert/strict";
import { test } from "node:test";

import {
  PREFIX,
  answer,
  mapValue,
  moduleLabels,
  platformLabels,
  sections,
  versionLabels,
} from "./issue-labels.ts";

const BUG_BODY = `### Platforms affected

Windows

### Affected module

Network

### Version

1.2.3

### What happened?

The download stalled.`;

void test("splits a rendered body into sections", () => {
  const found = sections(BUG_BODY).map((section) => section.heading);
  assert.deepEqual(found, [
    "Platforms affected",
    "Affected module",
    "Version",
    "What happened?",
  ]);
});

void test("reads an answer case-insensitively", () => {
  assert.equal(answer(BUG_BODY, "platforms affected"), "Windows");
  assert.equal(answer(BUG_BODY, "Affected Module"), "Network");
});

void test("a blank answer is absent rather than an empty string", () => {
  assert.equal(answer("### Version\n\n_No response_", "Version"), "_No response_");
  assert.equal(answer("### Version\n\n", "Version"), undefined);
});

void test("maps platform answers to platform labels", () => {
  assert.deepEqual(platformLabels(BUG_BODY), [`${PREFIX.platform}windows`]);
});

void test("maps module answers to module labels", () => {
  assert.deepEqual(moduleLabels(BUG_BODY), [`${PREFIX.module}network`]);
});

void test("accepts a version with or without a leading v", () => {
  const withV = "### Version\n\nv0.1.0\n";
  assert.deepEqual(versionLabels(withV), [`${PREFIX.version}0.1.0`]);
  assert.deepEqual(versionLabels(BUG_BODY), [`${PREFIX.version}1.2.3`]);
});

void test("drops a version that is not a version number", () => {
  assert.deepEqual(versionLabels("### Version\n\nlatest\n"), []);
  assert.deepEqual(versionLabels("### Version\n\n1.2\n"), []);
});

void test("an unknown dropdown value produces no label", () => {
  assert.deepEqual(platformLabels("### Platforms affected\n\nSolaris\n"), []);
  assert.deepEqual(moduleLabels("### Affected module\n\nEverything\n"), []);
});

void test("mapValue ignores case and rejects misses", () => {
  const table = new Map([["macos", "macos"]]);
  assert.equal(mapValue(table, "macOS", "p: "), "p: macos");
  assert.equal(mapValue(table, "windows", "p: "), undefined);
  assert.equal(mapValue(table, undefined, "p: "), undefined);
});

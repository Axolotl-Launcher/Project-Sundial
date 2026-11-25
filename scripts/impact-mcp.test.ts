// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

import assert from "node:assert/strict";
import { test } from "node:test";

import { stripNextStepHint } from "./impact-mcp.ts";

void test("an unadorned JSON payload is unchanged", () => {
  const payload = '{"summary":{"changed_count":1}}';
  assert.equal(stripNextStepHint(payload), payload);
});

void test("the tool hint is removed before parsing", () => {
  const payload = '{"summary":{"changed_count":1}}\n\n---\n**Next:** Review affected processes.';
  assert.equal(stripNextStepHint(payload), '{"summary":{"changed_count":1}}');
});

void test("a literal Next inside JSON is not treated as the hint", () => {
  const payload = '{"message":"**Next:** is just text"}';
  assert.equal(stripNextStepHint(payload), payload);
});

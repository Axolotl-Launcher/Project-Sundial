// SPDX-FileCopyrightText: 2026 The Superlogical Authors
// SPDX-License-Identifier: GPL-3.0-only

// Keeps the issue forms and the label mapping in step.
//
// A dropdown option with no entry in the mapping is silently ignored: the
// issue opens, nobody notices, and the label is simply absent. These tests
// read the forms and fail when the two disagree.

import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";

import { parse } from "yaml";

import {
  HEADING,
  moduleLabels,
  platformLabels,
} from "./issue-labels.ts";

const BUG_FORM = ".github/ISSUE_TEMPLATE/bug-report.yml";
const FEATURE_FORM = ".github/ISSUE_TEMPLATE/feature-request.yml";

interface FormOption {
  readonly id: string;
  readonly attributes: { readonly options?: readonly string[] };
}

interface Form {
  readonly body: readonly FormOption[];
}

function load(path: string): Form {
  return parse(readFileSync(path, "utf8")) as Form;
}

function optionsOf(form: Form, id: string): readonly string[] {
  const field = form.body.find((entry) => entry.id === id);
  assert.ok(field !== undefined, `form has no field "${id}"`);
  return field.attributes.options ?? [];
}

void test("every module option in the bug form is a known module", () => {
  const form = load(BUG_FORM);
  for (const option of optionsOf(form, "module")) {
    const body = `### ${HEADING.module}\n\n${option}\n`;
    assert.equal(
      moduleLabels(body).length,
      1,
      `module option "${option}" has no label mapping`,
    );
  }
});

void test("every module option in the feature form is a known module", () => {
  const form = load(FEATURE_FORM);
  for (const option of optionsOf(form, "module")) {
    const body = `### ${HEADING.module}\n\n${option}\n`;
    assert.equal(
      moduleLabels(body).length,
      1,
      `module option "${option}" has no label mapping`,
    );
  }
});

void test("the two forms offer the same module options", () => {
  assert.deepEqual(
    optionsOf(load(BUG_FORM), "module"),
    optionsOf(load(FEATURE_FORM), "module"),
  );
});

void test("every platform option in the bug form is a known platform", () => {
  const form = load(BUG_FORM);
  for (const option of optionsOf(form, "platforms")) {
    const body = `### ${HEADING.platform}\n\n${option}\n`;
    assert.equal(
      platformLabels(body).length,
      1,
      `platform option "${option}" has no label mapping`,
    );
  }
});

void test("the form headings the mapping expects are the ones the forms use", () => {
  const bug = load(BUG_FORM);
  const headings = bug.body.map((entry) => entry.id);
  assert.ok(headings.includes("platforms"), "bug form lost its platform field");
  assert.ok(headings.includes("module"), "bug form lost its module field");
  assert.ok(load(FEATURE_FORM).body.some((entry) => entry.id === "module"));
});

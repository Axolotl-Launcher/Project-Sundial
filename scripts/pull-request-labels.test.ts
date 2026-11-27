// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

// Tests for path-to-module mapping.

import assert from "node:assert/strict";
import { test } from "node:test";

import { modulesForPaths } from "./pull-request-labels.ts";

void test("a system crate maps to its module", () => {
  assert.deepEqual(modulesForPaths(["apps/core/network/src/lib.rs"]), ["network"]);
  assert.deepEqual(modulesForPaths(["apps/core/content/mod_metadata/src/lib.rs"]), [
    "mod-metadata",
  ]);
  assert.deepEqual(modulesForPaths(["apps/core/content/resources/src/lib.rs"]), [
    "resources",
  ]);
  assert.deepEqual(modulesForPaths(["apps/core/content/src/lib.rs"]), ["content"]);
});

void test("the aggregator maps to domain", () => {
  assert.deepEqual(modulesForPaths(["apps/core/src/lib.rs"]), ["domain"]);
});

void test("ui and frontend are distinct modules", () => {
  assert.deepEqual(modulesForPaths(["apps/ui/src/lib.rs"]), ["ui"]);
  assert.deepEqual(modulesForPaths(["apps/frontend/src/lib.rs"]), ["frontend"]);
});

void test("platform shells map to platforms", () => {
  assert.deepEqual(modulesForPaths(["platforms/windows/src/main.rs"]), [
    "platforms",
  ]);
});

void test("tests, scripts and CI map to repository", () => {
  assert.deepEqual(modulesForPaths(["scripts/turbo.ts"]), ["repository"]);
  assert.deepEqual(modulesForPaths([".github/workflows/ci.yml"]), ["repository"]);
  assert.deepEqual(modulesForPaths(["tests/tests/spi_contract.rs"]), [
    "repository",
  ]);
});

void test("root manifests map to repository", () => {
  assert.deepEqual(modulesForPaths(["Cargo.toml"]), ["repository"]);
  assert.deepEqual(modulesForPaths(["turbo.jsonc"]), ["repository"]);
});

void test("several modules are deduplicated and sorted", () => {
  assert.deepEqual(
    modulesForPaths([
      "apps/core/network/src/lib.rs",
      "apps/ui/src/lib.rs",
      "apps/core/network/src/other.rs",
      "apps/frontend/src/lib.rs",
    ]),
    ["frontend", "network", "ui"],
  );
});

void test("an unknown path yields nothing", () => {
  assert.deepEqual(modulesForPaths(["README.md"]), []);
});

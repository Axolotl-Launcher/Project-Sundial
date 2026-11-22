<!-- SPDX-FileCopyrightText: 2026 Axolotl Team -->
<!-- SPDX-License-Identifier: GPL-3.0-only -->

# Tests

Every test lives under this directory. A `src` directory holds production code
only; the one exception is the `#[path]` declaration that attaches a file from
here.

## Unit tests

A unit test belongs to the source file it exercises, so its path mirrors that
file under `tests/`:

```
apps/core/launch/src/flow.rs  ->  tests/apps/core/launch/src/flow.rs
```

Attach it from the source module:

```rust
#[cfg(test)]
#[path = "../../../../tests/apps/core/launch/src/flow.rs"]
mod tests;
```

The module stays a child of the source module, so it keeps access to private
items. When one source file has several test modules, the first uses the
mirrored file and the others live in a directory named after it, for example
`tests/apps/core/launch/src/chain/document_tests.rs`.

A test-only helper shared by several modules follows the same rule: it lives
here and the source module re-exports it under `#[cfg(test)]`.

## Integration and policy tests

- Repository-wide policy tests belong to the `axlauncher_policy_tests` package
  and live in `tests/tests/`.
- A test that needs only a crate's public API lives under
  `tests/apps/<crate path>/tests/` and is declared as a `[[test]]` target in
  `Cargo.toml`.

## Rules

- Do not add a `#[cfg(test)] mod tests { ... }` block inside `src`.
- Do not put tests in a package's own `tests/` directory outside this tree.
- Keep each test file under the 500-line limit; split by module or by scenario.

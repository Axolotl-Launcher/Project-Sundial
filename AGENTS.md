<!-- SPDX-FileCopyrightText: 2026 Axolotl Team -->
<!-- SPDX-License-Identifier: GPL-3.0-only -->

# Working in Axolotl Launcher

This guide applies to the entire Rust + gpui-kit workspace. Axolotl is a fresh
implementation. Write original code and follow the style of the owning crate.

Within higher-priority constraints, the user's explicit task instructions take precedence over skill and project-file guidelines.
Write all agent-facing documentation in English.

See [.agents/AGENTS.md](.agents/AGENTS.md) for agent documentation and temporary work.

## Start here

1. Run `git status --short --branch` from the repository root. Preserve existing
   work when editing a shared checkout.
2. Identify the owner and existing API in the [crate boundaries](.agents/docs/crates.md)
   before adding a capability or dependency. For a first build, follow [CONTRIBUTING.md](CONTRIBUTING.md#requirements)
   and run `pnpm install --frozen-lockfile`. The Rust pin is in
   [rust-toolchain.toml](rust-toolchain.toml). Node and pnpm requirements are in
   [package.json](package.json).
3. Use GitNexus for code navigation. Before editing, run upstream impact analysis
   for the target and report callers, affected processes and risk. Warn before
   editing a HIGH or CRITICAL target. Confirm UNKNOWN results with a text search.
   Follow the [GitNexus guide](.agents/docs/gitnexus.md) for first-time setup,
   renames and the required pre-commit change analysis.

## Where changes belong

Each resource has an owner. Use its existing API or SPI port. Only the owner
may introduce direct resource access. Inject providers at the frontend.
The [crate boundaries](.agents/docs/crates.md) define the allowed dependencies,
resource access and narrow shared helpers. If a proposed improvement crosses
a boundary, follow the [approval process](.agents/docs/crates.md#proposing-a-boundary-change).

Keep domain and SPI independent of implementations, core independent of UI,
and platform entry points thin. Declare
new dependencies in the root workspace and inherit them in the consuming crate.
New crates inherit workspace lints, Rust version and licence. Upgrade gpui-kit
and its pinned gpui-pre together.

**Launcher data.** Axolotl reads and writes its own launcher schema. Foreign
launcher conversion belongs in an independent tool. An unknown instance
`schema_version` must yield `DiscoveryError::UnsupportedSchema`. The
[path policy](tests/tests/external_formats.rs) and
[instance tests](apps/core/state/src/instance/tests.rs) cover these boundaries.
For Minecraft's own manifests, [game format notes](docs/game-format-changes.md)
explain which fields select each launch path.

## Code and review

- Express constraints with types or lints. Where those cannot carry a rule,
  write a test. Make CI fail on violations and wire new tooling into an actual task.
- Give each function one job, prefer early returns, use meaningful names and
  name magic numbers. Remove dead code. Reserve comments for non-obvious reasons
  or external constraints. Give doc comments a meaningful opening, as checked
  by the [comment policy](tests/tests/comment_policy.rs).
- Keep functions within 60 lines and nesting within three levels. Split tracked
  code files over 500 lines by role, including tests. Explain lint suppressions.
  Workspace lints cover unsafe code, `dbg!`, `todo!` and placeholder names.
  See [Clippy thresholds](clippy.toml) and [workspace lints](Cargo.toml).
- Write scripts in strippable TypeScript. Keep machine paths, credentials, logs
  and mirror endpoints out of the repository. For registry access see the
  [mirror instructions](README.md#网络) and preserve the resolved lockfile.

## Verification

During development, target the change. For example, use
`cargo nextest run -p axlauncher_state` for persistence,
`cargo nextest run -p axlauncher_policy_tests` for SPI and repository contracts,
or `pnpm test:scripts` for scripts.

Before reporting completion, run the following from the repository root.
Report any check that failed or could not run and give the reason.

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo nextest run --workspace --locked
cargo shear
cargo deny check
typos
tombi format --check
tombi lint
reuse lint
```

`pnpm check` also exercises host-filtered Turbo tasks, script checks and hooks.
The workspace-wide Rust commands above cover crates outside those filters.

## Commits

Use an English Conventional Commit with an imperative subject and no trailing
period. Use `feat`, `fix`, `refactor`, `docs`, `build`, `ci`, `test` or `chore`.
Include a DCO `Signed-off-by` and an `Assisted-by` trailer for agent
contributions. Reserve `Reviewed-by` and `Tested-by` for actual human work.
Keep [prek hooks](CONTRIBUTING.md#git-hooks) enabled. Fix a failing check at
its source. If the hook itself is wrong, fix `prek.toml` in the same commit.

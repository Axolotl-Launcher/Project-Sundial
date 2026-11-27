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
   [package.json](package.json). For cross-crate navigation, the optional local
   CodeGraph index in [CONTRIBUTING.md](CONTRIBUTING.md#code-graph) answers
   caller and impact questions without a file-by-file search.
3. Before editing, search for the target's callers with `rg` and read the
   surrounding code.

## Where changes belong

Keep tests under `tests/` and mirror the path to the source. Follow the
layout and module rules in [tests/AGENTS.md](tests/AGENTS.md).

Each resource has an owner. Use its existing API or SPI port. Only the owner
may introduce direct resource access. Route desktop installation and launch
through the typed services in `axlauncher_core::desktop`; the frontend owns
UI tasks, not provider dispatch or persistence.
The [crate boundaries](.agents/docs/crates.md) define the allowed dependencies,
resource access and narrow shared helpers. If a proposed improvement crosses
a boundary, follow the [approval process](.agents/docs/crates.md#proposing-a-boundary-change).
Follow the [filesystem path policy](.agents/docs/filesystem-paths.md) for new
path configuration and launcher-managed layouts; use the storage APIs instead
of building those paths at call sites.

Keep domain and SPI independent of implementations, core independent of UI,
and platform entry points thin. Declare
new dependencies in the root workspace and inherit them in the consuming crate.
New crates inherit workspace lints, Rust version and licence. Upgrade gpui-kit
and its pinned gpui-pre together.

**Launcher data.** Axolotl reads and writes its own managed launcher schema.
The modpack plugin may read a user-selected MultiMC instance export ZIP as a
distribution input. Converting installed foreign instances, launcher settings
or custom launch components belongs in an independent tool. An unknown instance
`schema_version` must yield `DiscoveryError::UnsupportedSchema`. The
[path policy](tests/tests/external_formats.rs) and
[profile tests](tests/apps/core/state/src/profile.rs) cover these boundaries.
For Minecraft's own manifests, [game format notes](docs/game-format-changes.md)
explain which fields select each launch path.

## Engineering discipline

These rules decide what gets built and what gets deleted. They outrank a
habit carried over from another project.

1. **No backward compatibility.** Delete what is obsolete. Do not add a
   compatibility layer, a migration script or a fallback path.
2. **Simplest implementation that meets the need.** No preventive abstraction
   and no configuration layer kept for a use case that does not exist yet.
3. **Grow in layers.** Get a minimal end-to-end path working, then add to it.
   Never dismantle something that works today because complexity is expected
   later.
4. **Keep components modular and concerns separate.**
5. **Prefer a maintained third-party library.** Do not rewrite what a mature
   library already does well without a specific reason.
6. **Survey what the current dependencies already provide** before adding a
   crate or writing it yourself. Do not assume a library lacks the capability.
7. **Architect for the long term.** A stopgap accepted as temporary is not an
   option.
8. **Study how mature products solve the same problem** and use a proven
   pattern instead of inventing one.

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
- Keep modules deep and interfaces narrow. Run `pnpm architecture` to check
  them with `deslop-please`. The CI gate covers the Linux host target with
  default features. Fix findings at their cause and remove resolved debt from
  `.deslop-please.allow`, including access from modules that no longer use a
  leaked field. The baseline is reserved for debt that existed when the check
  was introduced.
- Write scripts in strippable TypeScript. Keep machine paths, credentials, and
  logs out of the repository. For registry access see the
  [mirror instructions](CONTRIBUTING.md#cargo-registry-mirrors) and preserve
  the resolved lockfile.

## Verification

During development, target the change. For example, use
`cargo nextest run -p axlauncher_state` for persistence,
`cargo nextest run -p axlauncher_policy_tests` for SPI and repository contracts,
or `pnpm test:scripts` for scripts.

Before reporting completion, run the following from the repository root.
Report any check that failed or could not run and give the reason.

```sh
cargo fmt --all --check
node scripts/desktop.ts exec cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
node scripts/desktop.ts exec cargo nextest run --workspace --locked
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

<!-- SPDX-FileCopyrightText: 2026 The Superlogical Authors -->
<!-- SPDX-License-Identifier: GPL-3.0-only -->

# AGENTS.md

Instructions for anyone — human or agent — changing this repository. Read this
before your first edit.

## What this is

A Rust + gpui-kit application. Keep it small: this repository is a fresh
implementation, not a fork, so nothing here should be copied in from another
codebase.

## Layout

```
apps/core          superlogical_core            domain logic, no UI dependency
apps/ui            superlogical_ui             palette + allowed gpui-kit vocabulary
apps/frontend      superlogical_frontend       state, layout, window wiring
platforms/windows  superlogical_platform_windows  produces superlogical.exe
platforms/linux    superlogical_platform_linux    produces a Linux binary
```

Dependencies point one way only:

```
core <- ui
core <- frontend -> ui
platforms/* -> frontend
```

Breaking that shape is the main mistake to avoid:

- `ui` must not depend on `core` or on application state. It re-exports
  gpui-kit components and owns the palette; that is all.
- `frontend` is the only crate that combines domain logic with components.
- `platforms/*` contain no UI: they call `superlogical_frontend::run()`.
- Only `ui` may name `gpui_kit` paths. Everything else goes through
  `superlogical_ui`'s re-exports, so swapping the component system touches one
  crate.

## Code rules

These are binding. The table says how each one is enforced, because a rule that
only lives in prose gets broken silently.

**Express intent with code, and where code cannot express it, with a test.**
A constraint that a type, a signature, or a lint can carry must not be left to
a comment or to reviewer attention. If neither a type nor a lint can carry it,
write a test that fails when the constraint breaks.

| Rule | Enforced by |
|---|---|
| No `dbg!` or `todo!` left behind | `clippy::dbg_macro`, `clippy::todo` (deny) |
| Functions stay short | `clippy::too_many_lines` at 60 (warn; CI is `-D warnings`) |
| Shallow nesting, prefer early return | `clippy::excessive_nesting` at 3 |
| No one-letter or placeholder names | `clippy::disallowed_names`, `clippy::many_single_char_names` |
| Every lint suppression explains itself | `clippy::allow_attributes_without_reason` |
| No unsafe | `unsafe_code = "forbid"` |
| No unused dependencies | `cargo shear` |
| Doc comments do not open with a mechanical verb | `tests/tests/comment_policy.rs` |
| Scripts are TypeScript, never JavaScript | `scripts/check-language.ts` (`pnpm nojs`) |
| TypeScript must be strippable by Node | `tsc` with `erasableSyntaxOnly` (`pnpm tsc`) |
| Script style and magic numbers | ESLint (`pnpm eslint`) |

Rules no machine can decide, so a human reviewer must:

- **No magic numbers.** Name the constant. A lint cannot tell `px(720.)` from a
  legitimate literal, so this is review, not a check.
- **Comment only what the code cannot say.** The test catches the clear-cut
  case, `/// Returns the value` above `fn value()`; it cannot judge the rest.
  Keep a comment for a non-obvious reason, a workaround, or an external
  constraint, in one or two lines. Module-level `//!` docs describing a crate's
  role are fine.
- **One job per function.** The line limit catches the extremes; splitting a
  30-line function that does two things is still your call.
- **Delete dead code outright.** No commented-out blocks, no `_unused` bindings,
  no `#[allow(dead_code)]`. Version control remembers it.
- **Match the surrounding style.** Read the neighbouring code before adding a
  new pattern.

When you add a rule to this list, first try to express it as a lint, a type, or
a test in `tests/`. Only what survives that goes in the prose above.

## Git hooks

`prek.toml` defines the hooks every developer runs: `commit-msg` requires a DCO
sign-off, `pre-commit` runs the fast checks, `pre-push` runs clippy, the tests
and the dependency policy. CI's `Hooks` job runs the same configuration.

An agent must not bypass them with `--no-verify` or `PREK_NO_VERIFY`. If a hook
fails, fix the cause; if the hook itself is wrong, say so and change `prek.toml`
in the same commit rather than stepping around it.

## Verify before you report done

Run these from the repository root; all must pass.

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo nextest run --workspace
cargo shear          # no declared-but-unused dependencies
cargo deny check     # advisories, licences, duplicate versions, sources
typos
tombi format --check && tombi lint
reuse lint           # every file carries SPDX tags or a REUSE.toml entry
```

`pnpm check` runs the whole list except clippy and cargo-deny; `pnpm lint` and
`pnpm test` drive the same work through turbo.

If a check cannot run in your environment, say so explicitly in your report
rather than implying it passed.

## Conventions

- **Commits**: Conventional Commits (`feat:`, `fix:`, `refactor:`, `docs:`,
  `build:`, `ci:`, `test:`, `chore:`). Imperative subject, no trailing period.
- **Trailers**: `Signed-off-by` is required (DCO). Add `Assisted-by: <tool>` when
  an agent wrote part of the change. `Reviewed-by` / `Tested-by` may only be
  written when a human actually did it — never on your own initiative.
- **Dependencies**: add one only when a feature needs it, and add it to
  `[workspace.dependencies]` in the root `Cargo.toml`. `cargo shear` fails CI on
  unused entries.
- **Dependency direction**: see the diagram above. A new crate goes under
  `apps/` (library) or `platforms/` (executable shell) and must declare
  `[lints] workspace = true`, `rust-version.workspace = true`, and
  `license.workspace = true`.
- **gpui-kit**: upgrade it as a unit. `gpui-pre` is pinned exactly by gpui-kit
  because a newer snapshot can break the components; never override it alone.
- **Network**: if crates.io is unreachable, use the mirror config documented in
  `README.md` to resolve, but commit the resulting `Cargo.lock` unchanged and
  do not record endpoint details in the repository.

## Do not

- Do not add CI steps that only warn where a failure would catch the problem.
- Do not add build tooling that nothing invokes: a declared-but-unused cache
  wrapper, an unused release-cache warmer, or a changelog tool with no file it
  generates.
- Do not commit machine-specific details: absolute paths, proxy or mirror
  endpoints, local credentials, or logs from a developer machine.

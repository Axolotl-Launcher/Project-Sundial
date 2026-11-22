<!-- SPDX-FileCopyrightText: 2026 Axolotl Team -->
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
apps/core          axlauncher_core            domain logic, no UI dependency
apps/ui            axlauncher_ui             palette + allowed gpui-kit vocabulary
apps/frontend      axlauncher_frontend       state, layout, window wiring
platforms/windows  axlauncher_platform_windows  produces axlauncher.exe
platforms/linux    axlauncher_platform_linux    produces a Linux binary
```

Dependencies point one way only, and Cargo enforces it:

```
apps/core/* -> spi -> domain
apps/frontend -> ui, core
platforms/* -> frontend
```

Each system under `apps/core/` is its own crate. Callers depend on
`axlauncher_spi` ports, never on a concrete provider; `apps/frontend` picks
which provider to wire at startup. Adding a capability means adding a trait to
`spi` and an implementation in the owning system, not calling across systems.

## Compatibility boundaries

This project reads and writes only its own data formats. It must not grow an
importer, converter or migration layer for another launcher's data — not for
the reference implementation, and not for MultiMC, PCL, HMCL or anything else.

A foreign format is converted by a separate, independent tool that reads that
format and writes this project's format. Keeping the conversion outside means
this codebase has one schema and one set of invariants, with no compatibility
branches to test or to regress.

The structural half is enforced by `tests/tests/external_formats.rs`: no file
or directory under `apps/` or `platforms/` may be named for compatibility,
legacy data, migration, importing or conversion. The behavioural half is
`schema_version`: a manifest this build does not know is rejected with
`DiscoveryError::UnsupportedSchema`, never migrated in place.

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

<!-- gitnexus:start -->
# GitNexus — Code Intelligence

This project is indexed by GitNexus as **Axolotl-2.0**.

> Index stale? Run `node .gitnexus/run.cjs analyze --index-only` from the project root — it auto-selects an available runner. No `.gitnexus/run.cjs` yet? Bootstrap with `npx`, `bunx`, or `pnpm dlx` — e.g. `bunx gitnexus@latest analyze` (npm 11 npx crash; #1939).

## Always Do

- **MUST run impact before editing.** Use `impact({target: "symbolName", direction: "upstream"})` or `node .gitnexus/run.cjs impact "symbolName" --direction upstream --repo .`; report callers, processes, and risk. Never substitute grep for graph analysis.
- **MUST analyze graph changes before committing.** Use `detect_changes({scope: "all"})` (MCP) or `node .gitnexus/run.cjs detect-changes --scope all --repo .` (CLI fallback). `partial: true` or `truncated: true` is not a clean check — a zero means unseen, not unaffected; re-run it. For regression review: `detect_changes({scope: "compare", base_ref: "main"})` or `node .gitnexus/run.cjs detect-changes --scope compare --base-ref "main" --repo .`.
- MUST warn on HIGH/CRITICAL `risk` pre-edit; never use `riskSharedAxes` to waive a HIGH/CRITICAL `risk` warning. Compare File/symbol: MCP File omits axes; Graph-RAG expands File.
- **MUST treat `risk: UNKNOWN` as unresolved, not as low.** An empty caller set is not evidence the symbol is unused — it can also mean the callers are not resolvable by the index (plain-object property access, dynamic dispatch, cross-language calls). `impact` pairs `UNKNOWN` with a `riskNote` saying so. Confirm with a text search before treating the symbol as safe to change or delete; do not proceed on the strength of a zero.
- **MUST use `query({search_query: "concept"})` for concepts/flows, `context({name: "symbolName"})` for a named symbol, or `impact` for blast radius, on read-only callers, dependencies, imports, or execution flow.** Graph first; text search only for empty/`UNKNOWN`/literals.
- For security review, `explain({target: "fileOrSymbol"})` lists taint findings (source→sink flows; needs `analyze --pdg`).

## Never Do

- NEVER edit a function, class, or method before MCP/CLI impact analysis.
- NEVER ignore HIGH or CRITICAL risk warnings from impact analysis, and never read `UNKNOWN` as an all-clear — it means the walk could not answer, which is the one verdict that requires confirming by other means.
- NEVER rename symbols with find-and-replace — use `rename` which understands the call graph.
- NEVER commit before MCP/CLI graph change analysis.

## Resources

| Resource | Use for |
| --- | --- |
| `gitnexus://repo/Axolotl-2.0/context` | Codebase overview, check index freshness |
| `gitnexus://repo/Axolotl-2.0/clusters` | All functional areas |
| `gitnexus://repo/Axolotl-2.0/processes` | All execution flows |
| `gitnexus://repo/Axolotl-2.0/process/{name}` | Step-by-step execution trace |

## CLI

| Task | Read this skill file |
| --- | --- |
| Understand architecture / "How does X work?" | `.claude/skills/gitnexus-exploring/SKILL.md` |
| Blast radius / "What breaks if I change X?" | `.claude/skills/gitnexus-impact-analysis/SKILL.md` |
| Trace bugs / "Why is X failing?" | `.claude/skills/gitnexus-debugging/SKILL.md` |
| Rename / extract / split / refactor | `.claude/skills/gitnexus-refactoring/SKILL.md` |
| Tools, resources, schema reference | `.claude/skills/gitnexus-guide/SKILL.md` |
| Index, status, clean, wiki CLI commands | `.claude/skills/gitnexus-cli/SKILL.md` |

<!-- gitnexus:end -->

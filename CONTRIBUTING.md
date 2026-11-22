<!-- SPDX-FileCopyrightText: 2026 Axolotl Team -->
<!-- SPDX-License-Identifier: GPL-3.0-only -->

# Contributing

## Requirements

| Tool | Version | Notes |
|---|---|---|
| Rust | 1.100.0 | pinned by `rust-toolchain.toml`; `rustup` installs it automatically |
| Node | >= 24.22.0 | for pnpm + turbo only |
| pnpm | 12.18.0 | pinned by `packageManager` in `package.json` |
| Windows | any recent MSVC toolchain | gpui-pre uses Win32 + DirectWrite + DirectX |
| Linux | fontconfig, X11/Wayland headers, Vulkan driver | needed to build |

Additional Rust tools used by the checks, install once:

```bash
cargo install cargo-nextest cargo-shear cargo-deny --locked
# typos and tombi are available from most package managers
```

## First build

```bash
pnpm install                                  # turbo
cargo build -p axlauncher_platform_windows  # debug binary
pnpm dev:windows                              # run it
```

`pnpm build` builds everything turbo sees, but only for the current OS: the root
scripts inject a `--filter` per platform, because turbo itself has no notion of
the host platform and would otherwise compile the Linux shell on Windows.

## Before opening a pull request

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo nextest run --workspace
cargo shear
cargo deny check
typos
tombi format --check && tombi lint
 lint
```

`pnpm check` bundles most of this. CI runs all of it on Ubuntu and Windows, and
a single `CI result` check must be green before merge.

## Dependency policy

`deny.toml` holds the policy:

- **Licences**: permissive licences plus MPL-2.0 and GPL-3.0-only (this
  workspace's own licence). A new copyleft dependency needs an explicit
  decision, not a quiet addition to the list.
- **Sources**: crates.io only. Git dependencies require an `allow-git` entry.
- **Advisories**: a yanked crate fails the build.
- Duplicate versions warn; they do not block.

`pnpm` applies a 7-day `minimumReleaseAge`, so a package published in the last
week will not install. If `pnpm install` refuses a version, either wait or pick
the newest version that is older than the cutoff — do not raise the policy to
work around it.

## Code rules

See . In short: express constraints with types, lints or tests
before comments; comment only what code cannot say; small single-purpose
functions; early returns; no dead code; meaningful names; no magic numbers.

## Git hooks

[prek](https://github.com/j178/prek) manages fast local hooks; the configuration
is `prek.toml`.

```bash
prek install          # once per clone
prek run --all-files  # pre-commit checks, every file
prek run --stage pre-push
```

| Stage | Runs |
|---|---|
| `commit-msg` | DCO sign-off present, message in English |
| `pre-commit` | whitespace and encoding, structured-file parsing, large files, secrets, cargo fmt, tombi format, the Rust line limit |
| `pre-push` | DCO trailers and GPG signature on the commits being pushed |

The hooks deliberately stop at what is cheap to fix while a commit is still
local. Clippy, the tests, cargo deny, cargo shear, typos, REUSE, tsc and ESLint
run in CI; `pnpm check` runs most of the same checks locally on demand. CI's
`Hooks` job runs the pre-commit stage over a full checkout, so a file skipped
locally is still checked there.

Install prek from the release installer or with `cargo install prek --locked`.

## Scripts

Scripts are TypeScript, run directly by Node's type stripping — no build step
and no transpiler dependency:

```bash
node scripts/turbo.ts test
pnpm tsc      # type check; also rejects syntax Node cannot strip
pnpm eslint
pnpm nojs     # rejects any tracked .js/.mjs/.cjs/.jsx file
```

`eslint.config.js` is the one JavaScript file allowed, because ESLint loads its
config before TypeScript tooling exists. It is allowlisted by exact path in
`scripts/check-language.ts`.

## Issues and pull requests

Issue forms (`.github/ISSUE_TEMPLATE/`) ask for the platform, the affected
module and the version. `.github/workflows/triage.yml` turns those answers into
`platform:`, `module:` and `version:` labels; pull requests get `module:` labels
from the paths they touch and `version:` from the workspace version, plus
`status: unreviewed` until a maintainer submits a review.

The label vocabulary lives in `scripts/sync-labels.ts`. Run it after changing
it:

```bash
gh auth login          # once
node scripts/sync-labels.ts
```

Every dropdown option must have an entry in `scripts/issue-labels.ts`;
`scripts/forms.test.ts` fails when one does not.

## Commits

Conventional Commits, `Signed-off-by` required, plus `Assisted-by:` when a tool
wrote part of the change. See `AGENTS.md` for the full convention.

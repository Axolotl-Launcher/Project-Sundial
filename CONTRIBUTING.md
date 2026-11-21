<!-- SPDX-FileCopyrightText: 2026 The Superlogical Authors -->
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
cargo build -p superlogical_platform_windows  # debug binary
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
reuse lint
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

[prek](https://github.com/j178/prek) manages the hooks and their tools; the
configuration is `prek.toml`.

```bash
prek install          # once per clone
prek run --all-files  # everything, every file
prek run --stage pre-push
```

| Stage | Runs |
|---|---|
| `commit-msg` | DCO sign-off present |
| `pre-commit` | whitespace, encoding, structured-file parsing, large files, secrets, cargo fmt, no-JS, tsc, ESLint, typos, REUSE, tombi |
| `pre-push` | clippy, cargo shear, cargo deny, nextest, GPG signature on pushed commits |

Install prek from the release installer or with `cargo install prek --locked`.
CI runs the same `prek.toml` in the `Hooks` job.

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

## Commits

Conventional Commits, `Signed-off-by` required, plus `Assisted-by:` when a tool
wrote part of the change. See `AGENTS.md` for the full convention.

<!-- SPDX-FileCopyrightText: 2026 Axolotl Team -->
<!-- SPDX-License-Identifier: GPL-3.0-only -->

# Contributing

This desktop Minecraft launcher uses Rust and [GPUI Kit](https://gpui-kit.com).
pnpm and Turbo orchestrate development tasks. Read [AGENTS.md](AGENTS.md)
before editing for code ownership and verification.

## Requirements

| Tool | Version | Notes |
|---|---|---|
| Rust | 1.100.0 | pinned by `rust-toolchain.toml`; `rustup` installs it automatically |
| Node | >= 24.22.0 | runs pnpm, Turbo, and the TypeScript scripts |
| pnpm | 12.18.0 | pinned by `packageManager` in `package.json` |
| nix | flakes enabled | optional; builds the artifact CI publishes for Linux |

| Platform | Build requirements | Desktop binary |
|---|---|---|
| Windows | MSVC toolchain for Win32, DirectWrite, and DirectX | `axlauncher.exe` |
| Linux | Fontconfig, FreeType, X11 and Wayland headers, Vulkan, D-Bus, and pkg-config. See the [CI setup](.github/actions/setup-desktop-rust/action.yml) for the Ubuntu package list. The flake declares the same set for `nix develop`. | ELF `axlauncher` |
| macOS | Xcode Command Line Tools for Metal. See the [packaging guide](packaging/macos/README.md) for the app bundle, signing, and notarisation. | Mach-O `axlauncher` |

Install the Cargo extensions used by the checks once:

```bash
cargo install cargo-nextest cargo-shear cargo-deny --locked
# typos and tombi are available from most package managers
```

## First build

From the repository root, install the script dependencies, then build and run
the launcher for your host platform:

```bash
pnpm install --frozen-lockfile
pnpm build
pnpm dev
```

The root `build`, `dev`, `test`, and `lint` scripts use
[`scripts/turbo.ts`](scripts/turbo.ts) to limit Turbo to `apps/*` and the
current OS's platform shell. To target a shell directly, use a command such as
`pnpm dev:windows` or `node scripts/desktop.ts build`. The desktop build
compiles and embeds both worker libraries for the host. To invoke Cargo
directly for a desktop package, run it through `node scripts/desktop.ts exec`
so the artifacts are available. The standalone converter does not need this step.
The workspace enables incremental Cargo builds in
[`.cargo/config.toml`](.cargo/config.toml).

The launcher stores settings and instances in `AXLauncher` under the system
data directory, and rebuildable files in `AXLauncher` under the system cache
directory. Set `AXLAUNCHER_DATA_DIR` to keep data, instances, cache, and scratch
files under one directory for development. The
[filesystem path policy](.agents/docs/filesystem-paths.md) documents directory
selection and session overrides.

### Nix

On Linux the flake builds the published artifact from a pinned toolchain:

```bash
nix develop                    # rust 1.100.0, node, and the native libraries
nix build .#axlauncher         # bin/axlauncher plus the desktop entry
nix run                        # build and launch
nix build .#axlauncher-legacy  # ELF for traditional distributions, see packaging/linux
```

Both binaries live under `__axlbin`: `newest` links against nixpkgs' glibc and
backs `packages.axlauncher` and the AppImage, `legacy` links against glibc 2.35
and replaces the store interpreter and rpath for native packages. The
`legacy-elf` check fails when that ELF references the store or needs glibc newer
than 2.35 (Ubuntu 22.04). Only `newest` and the wrapper are pushed to the cache;
`legacy` exists for CI packaging.

Builds of `main` are pushed to the public `axlmc` cache, so `nix run` downloads
instead of compiling. A user who trusts flake configuration gets the substituter
from `flake.nix`; otherwise pass the settings explicitly:

```bash
nix run --extra-substituters https://axlmc.cachix.org \
  --extra-trusted-public-keys 'axlmc.cachix.org-1:dmsRJqFQqLMfvRveerHV3DxDCW1wq1N+TX1+dryYWZU='
```

The nix build repeats the three cargo invocations that
[`scripts/desktop.ts`](scripts/desktop.ts) performs; changing the embedded worker
libraries means changing both, and [the Linux packaging
notes](packaging/linux/README.md) record why.

### Cargo registry mirrors

If your network blocks direct access to crates.io, configure a mirror in your
user-level `~/.cargo/config.toml` or specify it for a single build:

```bash
cargo build --locked --config 'source.crates-io.replace-with="mirror"' \
                     --config 'source.mirror.registry="sparse+<mirror-url>/index/"'
```

Keep the committed `Cargo.lock` and leave local mirror settings out of the
repository.

### Launcher download mirrors

The desktop launcher has built-in endpoints for BMCLAPI, Minecraft Mirror
(MCIM), Aliyun Maven, and Fallen-Proxy.

In **Development → Download debug**, **Automatic** uses upstream for
installation data without an independent digest. **Trust built-in mirror**
also permits those inputs from the mirror for that category.
MCIM still follows the upstream source. Fallen-Proxy has a separate opt-in.

### CurseForge modpack imports

Set `CURSEFORGE_API_KEY` in the gitignored `.env` at the workspace root before
building the launcher. Cargo embeds the key in the executable, including
release builds. A build environment variable named `CURSEFORGE_API_KEY` takes
precedence over the file. `AXLAUNCHER_CURSEFORGE_API_KEY` can supply a runtime
override. Builds without a key can still import packs with bundled files only.

The import waits for any files that need manual downloading. Use **Choose
file…** for those entries, then **Continue installation**. The same session
keeps its resolved file versions and the downloads already verified.

CurseForge data packs without a world target stay in the instance's
`datapacks/` folder. In vanilla Minecraft, add them through the new world's
Data Packs screen or copy them into `saves/<world>/datapacks/` for an existing
world. Files already bundled inside a world's directory keep that location.
[CurseForge data pack guide](https://support.curseforge.com/support/solutions/articles/9000267956)

### MultiMC exports with custom components

Use the standalone converter for a MultiMC export that includes an enabled
custom patch, a local library, a replacement client JAR, or JAR mods.

```sh
cargo run --locked -p axlauncher_mmc_convert -- path/to/export.zip path/to/AXLauncher
```

The second argument is the Axolotl data directory shown in the desktop app.
The converter prepares the stock game and loader, applies supported patches
in component order, and publishes an instance under `profiles/`. Reopen the
launcher or reload its instance list to see it. The converted profile keeps
its private runtime under `.axl-runtime`.

Supported custom patch fields include the main class, flat game arguments,
a single Java major, dependency constraints, bundled Java libraries, a
bundled `mainJar`, and bundled ZIP or JAR entries in `jarMods`. A patch that
replaces a stock component or library, uses custom native libraries or
tweakers, or refers to an external artifact requires additional translation.

## Workspace layout

| Path | Responsibility |
|---|---|
| `apps/core/domain` and `apps/core/spi` | Shared domain values and provider interfaces |
| `apps/core` | System crates for installation, launch, persistence, networking, and other launcher services. The root crate re-exports these systems. |
| `apps/ui` | Palette, theme installation, and the GPUI Kit components used by the launcher |
| `apps/frontend` | Application state, layouts, windows, and provider selection |
| `platforms/*` | Thin executable entry points for Windows, Linux, and macOS |
| `tests` | Repository policy and cross-crate contract tests |

Core callers use SPI ports such as `Network`, `Storage`, and `Launcher`.
The system crates implement those ports and the frontend selects providers at
startup. Asynchronous ports use return-position `impl Trait` to avoid boxing
every call. The [SPI contract tests](tests/tests/spi_contract.rs) exercise
callers with interchangeable providers. See the
[crate ownership guide](.agents/docs/crates.md) for the full crate inventory,
permitted dependencies, and shared helpers.

## Before opening a pull request

```bash
cargo fmt --all --check
node scripts/desktop.ts exec cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
node scripts/desktop.ts exec cargo nextest run --workspace --locked
cargo shear
cargo deny check
typos
tombi format --check
tombi lint
reuse lint
pnpm architecture
```

`pnpm check` runs the host-filtered Turbo checks, formatters, script checks,
and hooks. The Cargo commands above check the full workspace. CI runs the
[repository checks](.github/workflows/repo-check.yml) on Ubuntu, then runs
Clippy and Nextest on Ubuntu, Windows, and macOS, and the
[architecture check](.github/workflows/ci.yml) on Ubuntu. The
[`CI result` job](.github/workflows/ci.yml) reports every stage.

## Architecture check

[deslop-please](https://github.com/deterministic-machines/deslop-please) enforces
the "deep modules, narrow interfaces" rule from *A Philosophy of Software
Design*. Install `rust-analyzer` with `rustup component add rust-analyzer`. The
architecture script applies the repository's field-leak patch to the pinned
`deslop-please` 0.1.4 source, then builds it with `cargo install --locked`. CI and
`pnpm architecture` use the same script and patch.

The CI gate covers the Linux host target with default features. The local check
uses the current Cargo target and default features. Run it on Linux to reproduce
the CI scope.

```bash
pnpm architecture             # Check against the baseline
node scripts/architecture.ts  # Show every finding, including existing debt
```

`.deslop-please.allow` records the findings that existed when the check was
introduced. For field leaks it records the field and the modules that access
it. A finding outside the baseline fails CI, including a new module accessing
an already listed field. Fix the cause and remove any resolved entries or
module names from the baseline.

The tool checks for module cycles, tangles, field leaks, shallow modules,
directories with too many Rust files or subdirectories, and long Rust files.

## Dependency policy

[`deny.toml`](deny.toml) defines the Rust dependency policy:

- **Licences**: permissive licences plus MPL-2.0 and GPL-3.0-only (this
  workspace's own licence). Adding a copyleft dependency outside the allowlist
  requires an explicit decision.
- **Sources**: crates.io only. Git dependencies require an `allow-git` entry.
- **Advisories**: a yanked crate fails the build.
- Duplicate versions warn; they do not block.

`pnpm` applies a 7-day `minimumReleaseAge`, so a package published in the last
week will not install. If `pnpm install` refuses a version, wait or choose a
version older than the cutoff. Dependabot covers Cargo, npm, and GitHub Actions.
Pin GitHub Actions to full commit SHAs.

### GPUI Kit upgrades

The workspace uses `gpui-kit 0.10.0`, with `gpui-pre 0.3.12` as its pinned
Zed GPUI snapshot. The same release family provides `gpui-base` for component
behaviour, `gpui-component` for styled components, and `gpui-kit-assets` for
Lucide icons. Upgrade GPUI Kit and its pinned GPUI snapshot together so the
components remain compatible. `apps/ui` owns the direct dependency. Other
crates use the `axlauncher_ui` re-exports.

## Code rules

Follow the [code and review rules](AGENTS.md#code-and-review). Use types, lints,
or tests to enforce constraints. Keep functions small and single-purpose, use
early returns and meaningful names, and comment only what the code cannot say.
Every crate inherits workspace lints with `[lints] workspace = true`. Keep the
Rust pin in `rust-toolchain.toml`, `rust-version` in `Cargo.toml`, and `msrv`
in `clippy.toml` in sync when updating Rust.

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
| `pre-commit` | whitespace and encoding, structured-file parsing, large files, secrets, cargo fmt, tombi format, the 500-line limit |
| `pre-push` | DCO trailers on the commits being pushed |

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
pnpm file-length  # rejects any code file over 500 lines
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

`version:` labels are the exception: the triage scripts create them on demand,
because a report can name a version that has no release yet.

Every dropdown option must have an entry in `scripts/issue-labels.ts`;
`scripts/forms.test.ts` fails when one does not.

## Licence and REUSE

The workspace is licensed under [GPL-3.0-only](LICENSES/GPL-3.0-only.txt).
Add SPDX metadata in file headers or in [`REUSE.toml`](REUSE.toml) for files
such as the root README, JSON files, and generated lockfiles. Run
`reuse lint` to verify coverage.

## Commits

Conventional Commits, `Signed-off-by` required, plus `Assisted-by:` when a tool
wrote part of the change. See `AGENTS.md` for the full convention.

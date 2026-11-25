<!-- SPDX-FileCopyrightText: 2026 Axolotl Team -->
<!-- SPDX-License-Identifier: GPL-3.0-only -->

# Contributing

This desktop Minecraft launcher uses Rust and [GPUI Kit](https://gpui-kit.com).
pnpm and Turbo orchestrate development tasks. Read [AGENTS.md](AGENTS.md)
before editing for code ownership, impact analysis, and verification.

## Requirements

| Tool | Version | Notes |
|---|---|---|
| Rust | 1.100.0 | pinned by `rust-toolchain.toml`; `rustup` installs it automatically |
| Node | >= 24.22.0 | runs pnpm, Turbo, and the TypeScript scripts |
| pnpm | 12.18.0 | pinned by `packageManager` in `package.json` |

| Platform | Build requirements | Desktop binary |
|---|---|---|
| Windows | MSVC toolchain for Win32, DirectWrite, and DirectX | `axlauncher.exe` |
| Linux | Fontconfig, FreeType, X11 and Wayland headers, Vulkan, D-Bus, and pkg-config. See the [CI setup](.github/actions/setup-desktop-rust/action.yml) for the Ubuntu package list. | ELF `axlauncher` |
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
`pnpm dev:windows` or `cargo build -p axlauncher_platform_windows`.
The workspace enables incremental Cargo builds in
[`.cargo/config.toml`](.cargo/config.toml).

The launcher stores settings and instances in `AXLauncher` under the system
application data directory by default. Set `AXLAUNCHER_DATA_DIR` to use a
different directory or reuse an existing data directory.

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

Set the corresponding environment variable before starting the desktop launcher.
Use a bare HTTPS hostname, without a scheme, port, or path. An unset or invalid
value leaves that integration unavailable.

| Environment variable | Host to configure |
| --- | --- |
| `AXLAUNCHER_BMCLAPI_HOST` | [BMCLAPI](https://bmclapidoc.bangbang93.com/) |
| `AXLAUNCHER_MCIM_HOST` | [Minecraft Mirror](https://www.mcimirror.top/guide/start/getting-started) |
| `AXLAUNCHER_ALIYUN_MAVEN_HOST` | [Aliyun Maven Central mirror](https://developer.aliyun.com/mvn/guide) |
| `AXLAUNCHER_FALLEN_PROXY_DOMAIN` | The base domain shared by the service subdomains in the [Fallen-Proxy guide](https://blog.fallenbreath.me/zh-CN/2025/minecraft-service-proxy) |

In **Development → Download debug**, **Automatic** uses upstream for
installation data without an independent digest. **Trust configured mirror**
also permits those inputs from the configured mirror for that category.
MCIM still follows the upstream source. Fallen-Proxy has a separate opt-in.

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
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo nextest run --workspace --locked
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

<!-- SPDX-FileCopyrightText: 2026 Axolotl Team -->
<!-- SPDX-License-Identifier: GPL-3.0-only -->

# Crate ownership and resource access

Use this contract when adding a dependency, an I/O operation or a public API.
The access rules apply to production code. Tests may create fixtures, inject
doubles and instantiate concrete providers to verify their SPI contracts.

## Resource owners

| Resource | Direct access belongs to | Entry point for callers |
| --- | --- | --- |
| HTTP and DNS | `network` owns HTTP clients, DNS routing, the OAuth loopback listener and transport error classification. | Inject `Network` for metadata and downloads, or `auth::AuthNetwork` for credential-bearing exchanges. |
| Launcher data paths, blob storage and atomic writes | `storage` resolves directory defaults and overrides, and owns `DataDir`, `GameDir`, `RootedStorage` and `write_atomic`. | Use the [filesystem policy](filesystem-paths.md) and the storage path APIs rather than discovering or constructing managed locations. Use `Storage` for blobs and `write_atomic` for owned documents. |
| Minecraft accounts and system credentials | `state` owns account records, Microsoft and Yggdrasil authentication, and session refresh. `storage` owns the platform credential store. | Use `state::accounts::AccountService` with injected `auth::CredentialStore` and `auth::AuthNetwork` providers. |
| Settings and instance manifests | `state` owns settings, `.axlprofile` creation and discovery, install status and runtime selections. | Use `load_settings` and `update_settings` for settings. Use `profile::create`, `profile::discover` and `profile::scan_resources` for instances; use `profile::mark_installed` or `profile::mark_failed` for install outcomes. `StateStore` serves the separate ID-based `InstanceRecord` API. |
| File downloads | `download_engine` schedules connections and ranges, manages shared bandwidth and buffers, tries alternate sources and verifies files before promotion. | Pass file requests and an injected `Network` to `download_engine::download_all` or `download_engine::download_all_reporting`. Use `DownloadEngine` for explicit policy and source ordering. |
| Installation files and asset indexes | `install` checks for missing libraries and assets, handles Minecraft metadata, and prepares the pinned third-party authentication agent. | Use `prepare::prepare` for game files and `auth_agent::prepare` for the Java agent. Both use `download_engine`. The SPI `Installer` accepts an `InstallPlan`. Its current `StepInstaller` implementation is a stub. |
| Mod loader metadata services | `loader` lists versions and resolves version profiles through an injected `Network`. | `loader::provider` selects the `ModLoader` implementation for both catalog and install. `install` places the resulting profile and handles installation processors. |
| Minecraft version naming | `mc_version` classifies Mojang catalog entries and matches the Maven versions published for each Forge or NeoForge game line. It has no I/O. | Call `core::mc_version::classify_java` for catalog classification. The loader uses `MavenTarget` for both listing and installation. Standalone tools may explicitly wrap `BuiltinJavaVersionParser` with the SPI fallback; the launcher installs no fallback parser. |
| Game version documents, classpath, JARs and natives | `launch` reads version files and JARs, evaluates launch rules and extracts natives. | Use `flow::launch` for prepared installations. Installation planning reuses `version::Version`, the library model and `rules::Target` to decide what to download. |
| Installation processors | `install` plans and runs the Java processors declared in a version profile. | Use `install::processors` for installation steps. Use `Launcher` to start a game. |
| Java discovery and game processes | `launch` provides `SystemJava` and `ProcessLauncher`. | Resolve the runtime through `Java::executable`. Pass the prepared `LaunchCommand` to `Launcher::launch`. |
| Event output | `telemetry` implements the event sink. | Call `Telemetry::record(Event)`. Command metrics belong to `observability`. |
| Desktop UI and startup environment | `ui` directly depends on gpui-kit. `frontend` wires the window, displays the active paths, and passes directory changes to storage. | Frontend code imports components through `axlauncher_ui` and calls system APIs through `core`. Platform shells call `frontend::run`. |
| Selected modpack archives | `modpack` reads the user-selected archive and interprets the manifest and payload, including portable MultiMC instance exports with standard game components. | The [modpack service](../../apps/frontend/src/modpacks/service.rs) invokes the built-in plugin, executes its typed plan through the existing system APIs, and publishes the new profile through `state`. It accepts a progress callback. The desktop view owns subscriptions and presentation. |

The frontend passes a `DataDir` snapshot to desktop workers. Instance discovery
and manifest access belong to `state`.

Use `core::desktop` for the workflows that span several owners: profile version
catalogs, game installation and launch. It wires providers, applies settings,
and sequences the owner calls, but does not implement loader protocols, file
preparation, profile writes, or process execution. GPUI workers and channels,
progress redraw throttling, and view state stay in `frontend`.

## Download protocol behavior

File transfers use HTTP/1.1 by default, including range requests. HTTP/2 is
available through an explicit download policy. Metadata negotiates HTTP normally.
HTTP/1.1 downloads can split known files from 32 KiB, with an initial range
size of at least 16 KiB. Under contention, the scheduler weights each file's
share by its remaining bytes and applies changes as connections are released.
HTTP/2 retains its 4 MiB range threshold and 8 MiB initial range. Only
standalone jobs probe for an unknown size. A range retry keeps bytes already
written when the server returns the requested interval and total size. Whole
file transfers read to the end of the response body without comparing the
written length with the HTTP `Content-Length` header. The staging file is
promoted after checking SHA-1 when supplied, or the expected size when only
size is supplied. The shared engine defaults to
256 connections globally and 16 per file. The desktop setting controls how
many files may be queued at once.

Direct lookups race weighted DoH upstreams, with two active per address family.
If neither family resolves within the DoH budget, the operating system
resolves the name. Single-label hosts and local DNS suffixes use the system
resolver directly. Applications can supply more suffixes through `DohConfig`.
The transport handles IP literals and proxy requests without a DNS-over-HTTPS
lookup for the destination.

## Workspace inventory

| Cargo package | Source | Owns |
| --- | --- | --- |
| `axlauncher_domain` | [domain](../../apps/core/domain/src/lib.rs) | Shared domain values such as the stable `Id`. Keep these free of I/O. |
| `axlauncher_spi` | [spi](../../apps/core/spi/src/lib.rs) | Provider traits, requests, results and errors shared across boundaries. Signatures use domain or SPI types. |
| `axlauncher_content` | [content](../../apps/core/content/src/lib.rs) | The installable content catalogue. `StaticProvider` implements `ContentProvider::list` and returns `ContentSummary` records. |
| `axlauncher_resources` | [resources](../../apps/core/resources/src/lib.rs) | User-managed resource records, currently `Favorite`. Downloadable game assets belong to install. |
| `axlauncher_loader` | [loader](../../apps/core/loader/src/lib.rs) | `ModLoader` providers that list versions and resolve profiles, currently Fabric, Quilt, Forge and NeoForge. |
| `axlauncher_mc_version` | [mc_version](../../apps/core/mc_version/src/lib.rs) | Pure version classification, build matching and ordering. It does not read foreign launcher data or decide how to install or launch a version. |
| `axlauncher_mod_metadata` | [mod_metadata](../../apps/core/mod_metadata/src/lib.rs) | Mod and loader metadata, currently the `Loader` enum. Game launch rules belong to launch. |
| `axlauncher_scheduler` | [scheduler](../../apps/core/scheduler/src/lib.rs) | Background work scheduling policy, currently priority `Band`. The originating system owns the work itself. |
| `axlauncher_observability` | [observability](../../apps/core/observability/src/lib.rs) | Command metrics and diagnostic records, currently `CommandMetric`. Telemetry owns event output. |
| `axlauncher_network` | [network](../../apps/core/network/src/lib.rs) | HTTP transport, retry classification and the shared download digest check. |
| `axlauncher_storage` | [storage](../../apps/core/storage/src/lib.rs) | Data directory layout, filesystem primitives and the byte storage provider. Document owners define schemas. |
| `axlauncher_state` | [state](../../apps/core/state/src/lib.rs) | Launcher settings and instance persistence, plus the in-memory `StateStore` implementation. |
| `axlauncher_download_engine` | [download_engine](../../apps/core/download_engine/src/lib.rs) | Execution and progress reporting for file downloads. |
| `axlauncher_install` | [install](../../apps/core/install/src/lib.rs) | Installation planning, metadata and preparation of game files before launch. |
| `axlauncher_launch` | [launch](../../apps/core/launch/src/lib.rs) | Building and executing a command from prepared files. JAR extraction uses the crate-local `NativeReader` interface. |
| `axlauncher_telemetry` | [telemetry](../../apps/core/telemetry/src/lib.rs) | Event sink implementation, currently `LoggingTelemetry`. |
| `axlauncher_core` | [core](../../apps/core/src/lib.rs) | Application-facing system re-exports and desktop request orchestration. Delegates each operation to its owning system or SPI provider. |
| `axlauncher_ui` | [ui](../../apps/ui/src/lib.rs) | Palette, theme installation and component re-exports. |
| `axlauncher_frontend` | [frontend](../../apps/frontend/src/lib.rs) | Root view, interaction state, windows and UI task lifecycle. |
| `axlauncher_modpack` | [modpack](../../apps/plugins/modpack/src/lib.rs) | Registered readers for modpack distribution inputs, plus file selection and override planning. Its only workspace dependency is SPI. |
| `axlauncher_mmc_convert` | [mmc-convert](../../tools/mmc-convert/src/main.rs) | Standalone conversion of selected MultiMC exports with custom launch components into native, isolated profiles. It uses the modpack archive reader and the core facade. |
| `axlauncher_platform_windows` | [Windows](../../platforms/windows/src/main.rs) | Windows executable entry point. |
| `axlauncher_platform_linux` | [Linux](../../platforms/linux/src/main.rs) | Linux executable entry point. |
| `axlauncher_platform_macos` | [macOS](../../platforms/macos/src/main.rs) | macOS executable entry point. |
| `axlauncher_policy_tests` | [tests](../../tests/tests) | Repository policies, architecture checks and cross-crate SPI contract tests. |

## Reusing and extending interfaces

1. Check the [SPI](../../apps/core/spi/src/lib.rs) and the resource owner above.
   Reuse an existing port or function when it already has the required contract.
   Keep interfaces used inside a single system local, as launch does with `NativeReader`.
2. For new behavior shared across systems, define the port in SPI, implement it
   in its owning system and inject it into consumers. Shared domain values can
   be used directly. Keep one implementation of each parser, integrity check
   or atomic write operation in its owning crate.
3. Core systems normally depend only on domain and SPI within the workspace.
   State and launch may depend on storage for managed paths and atomic writes. The download engine
   uses storage for staged file writes and network for digest verification. Install
   uses the download engine for planned files, storage for installation paths
   and atomic writes, network for metadata digest verification and launch for
   the shared version model and rules. The loader calls `mc_version` for
   Maven build resolution. Keep calls across these edges within the listed
   purposes.
4. When ownership changes, update this guide and the
   [dependency policy test](../../tests/tests/crate_boundaries.rs) together.
   The test inspects production and build dependency declarations, including
   optional, renamed and target-specific dependencies. Review standard library
   I/O and calls through permitted dependencies against the resource table.
   The [SPI contract tests](../../tests/tests/spi_contract.rs) exercise consumers
   with replaceable providers.

## Proposing a boundary change

If a better design would cross a documented boundary, explain the trade-off to
the user before changing the implementation, the dependency graph or its checks.

1. Name the exact rule, resource owner and callers involved. Describe the best
   design that fits the current contract, including which existing port or
   helper it would reuse and the concrete problem that would remain.
2. Describe the proposed ownership or interface change. Compare the two designs
   on the behavior the user needs, coupling, testing and maintenance. Give
   evidence for the expected benefit and identify the cost and risks.
3. Ask the user to approve the specific boundary change. Wait for explicit
   agreement before implementing it. If the user chooses the existing boundary,
   proceed with the design that fits it.
4. After approval, deliver the implementation, the revised ownership and caller
   rules in this guide, and the corresponding contract or policy tests in the
   same change. Update the root guide if a repository-wide rule also changes.

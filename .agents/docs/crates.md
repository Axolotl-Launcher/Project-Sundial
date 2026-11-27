<!-- SPDX-FileCopyrightText: 2026 Axolotl Team -->
<!-- SPDX-License-Identifier: GPL-3.0-only -->

# Crate ownership and resource access

Use this contract when adding a dependency, an I/O operation or a public API.
The access rules apply to production code. Tests may create fixtures, inject
doubles and instantiate concrete providers to verify their SPI contracts.

## Resource owners

| Resource | Direct access belongs to | Entry point for callers |
| --- | --- | --- |
| HTTP and DNS | `network` owns HTTP clients, DNS routing, the OAuth loopback listener and transport error classification. | Inject `network_spi::Network` for metadata and transfers or `network_spi::auth::AuthNetwork` for credential-bearing exchanges. |
| Launcher data paths, blob storage and atomic writes | `storage` resolves directory defaults and overrides, and owns `DataDir`, `GameDir`, `RootedStorage` and `write_atomic`. | Use the [filesystem policy](filesystem-paths.md) and the storage path APIs rather than discovering or constructing managed locations. Use `Storage` for blobs and `write_atomic` for owned documents. |
| Minecraft accounts and system credentials | `accounts` owns account records, Microsoft and Yggdrasil authentication, and session refresh. `storage` owns the platform credential store. | Use `core::desktop::accounts::DesktopAccounts`. The account worker receives the data root, uses the existing system store, and returns snapshots without credential references. |
| Settings and instance manifests | `state` owns settings, `.axlprofile` creation and discovery, install status and runtime selections. | Use `load_settings` and `update_settings` for settings. Use `profile::create`, `profile::discover` and `profile::scan_resources` for instances; use `profile::mark_installed` or `profile::mark_failed` for install outcomes. `StateStore` serves the separate ID-based `InstanceRecord` API. |
| File downloads | `download_engine` schedules connections and ranges, manages shared bandwidth and buffers, tries alternate sources and verifies files before promotion. | Inject `download_spi::FileDownloader`. The desktop client uses the shared download worker; the standalone converter explicitly injects `download_engine::Direct`. |
| Installation files and asset indexes | `install` checks for missing libraries and assets, handles Minecraft metadata, and prepares the pinned third-party authentication agent. | Use `prepare::prepare` for game files and `auth_agent::prepare` for the Java agent. Both take `FileDownloader`. Java runtime acquisition uses its separate existing transport. The SPI `Installer` accepts an `InstallPlan`. Its current `StepInstaller` implementation is a stub. |
| Embedded service libraries and processes | `worker` owns native loading, process and request lifetimes, and the local wire protocol. `storage` owns the scratch layout. | Desktop shells embed the three target artifacts and dispatch worker mode before GPUI. Desktop clients share the same process manager and never link to the worker implementations. |
| Mod loader metadata services | `loader` lists versions and resolves version profiles through an injected `Network`. | `loader::provider` selects the `ModLoader` implementation for both catalog and install. `install` places the resulting profile and handles installation processors. |
| Minecraft version naming | `mc_version` classifies Mojang catalog entries and matches the Maven versions published for each Forge or NeoForge game line. It has no I/O. | Call `core::mc_version::classify_java` for catalog classification. The loader uses `MavenTarget` for both listing and installation. Standalone tools may explicitly wrap `BuiltinJavaVersionParser` with the SPI fallback; the launcher installs no fallback parser. |
| Game version documents, classpath, JARs and natives | `launch` reads version files and JARs, evaluates launch rules and extracts natives. | Use `flow::launch` for prepared installations. Installation planning reuses `version::Version`, the library model and `rules::Target` to decide what to download. |
| Installation processors | `install` plans and runs the Java processors declared in a version profile. | Use `install::processors` for installation steps. Use `Launcher` to start a game. |
| Java runtimes and game processes | `java_discovery` enumerates system-wide candidates from storage's roots and registry results. `launch` validates paths, selects Java and provides `ProcessLauncher`. `state` owns the Java catalog. | Use `core::desktop::java` to list or discover runtimes. Resolve a runtime through `Java::executable`. Pass the prepared `LaunchCommand` to `Launcher::launch`. |
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

State persists one local Java list, per-major preferences and machine-local
preferences keyed by portable profile ID. Launch and installation check the
listed executables before use. The desktop Java catalog requests the discovery
worker explicitly or at application startup if the saved list is empty. The
worker only returns candidate paths. Core merges them with configured and
managed candidates through `SystemJava` after the scan succeeds. Listed
entries have the same operations regardless of how they were added. Storage
resolves discovery roots, Windows registry locations and the catalog paths.
Installation registers the specific runtime it provisions for Minecraft. Every
managed-runtime selection requires its completion record. Installers wait
asynchronously for the per-runtime writer lock and withdraw a damaged runtime's
record before repairing it.

Adding the first runtimes for a major or removing its preferred entry selects
and saves a preference. The selection favors host architecture, then standalone
or launcher-managed runtimes over runtimes declared by an enclosing application's
`product-info.json`. It compares releases and builds within each priority group.
Launch can fall back to another available listed runtime without changing the
saved preference.

On Windows, exploratory discovery follows PCL-CE's bounded breadth-first
search from fixed drives, Program Files and user directories. Registry and
Minecraft runtime locations supply additional candidates. Windows executable
lookup covers the working directory and PATH without starting a subprocess.
Automatic discovery excludes `WpSystem` and PCL-CE's excluded directories
before or after resolving a path. Explicit registrations retain their own
validation.

## Download protocol behavior

File transfers use HTTP/1.1 by default, including range requests. HTTP/2 is
available through an explicit download policy. Metadata negotiates HTTP normally.
HTTP/1.1 downloads can split known files from 32 KiB, with an initial range
size of at least 16 KiB. Under contention, the scheduler weights each file's
share by its remaining bytes and applies changes as connections are released.
HTTP/2 retains its 4 MiB range threshold and 8 MiB initial range. Only
standalone jobs probe for an unknown size. A ranged transfer uses a strong
ETag with `If-Range`, or a trusted whole-file digest. A range retry requests
the unwritten interval and validates the returned interval, total and any
established ETag. A conditional partial response may omit its ETag.
Without a trusted whole-file digest, the effective resource must also remain
the same across requests. Unsupported or inconsistent ranges
trigger a whole-file transfer from a clean stage. Whole-file transfers read
to EOF and check the manifest digest, or the expected size when only size is
supplied, before atomically publishing the staging file. A verification
mismatch allows one clean whole-file retry per source within the shared
request budget, after trying ready alternate sources.

Each file has three recovery requests per source across probes, ranges and
whole-file transfers. A recovery uses its budget when dispatched. Adjacent
failed intervals can share one recovery request after backoff. HTTP 429 and
503 with `Retry-After` cool down the submitted origin across jobs. HTTP 429
without a usable hint uses a short local cooldown. The engine tries ready
alternate sources and can resume a paused source from its own staging file.
Jittered exponential backoff uses the Tokio time driver and budgets up to
30 seconds of scheduled wait per file. Paused requests release connection
capacity while retaining their written prefixes.
The shared engine defaults to
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
| `axlauncher_spi` | [spi](../../apps/core/spi/src/lib.rs) | Launcher provider contracts for credentials, storage, content, loaders, version naming, Java, process launch and telemetry. |
| `axlauncher_network_spi` | [network SPI](../../apps/core/spi/network/src/lib.rs) | Buffered network requests, incremental HTTP transfers and authenticated HTTP exchanges. This crate has no workspace dependencies. |
| `axlauncher_download_spi` | [download SPI](../../apps/core/spi/download/src/lib.rs) | Integrity-checked file and batch downloads. Uses network SPI for the transfer protocol. |
| `axlauncher_content` | [content](../../apps/core/content/src/lib.rs) | The installable content catalogue. `StaticProvider` implements `ContentProvider::list` and returns `ContentSummary` records. |
| `axlauncher_resources` | [resources](../../apps/core/content/resources/src/lib.rs) | User-managed resource records, currently `Favorite`. Downloadable game assets belong to install. |
| `axlauncher_loader` | [loader](../../apps/core/loader/src/lib.rs) | `ModLoader` providers that list versions and resolve profiles, currently Fabric, Quilt, Forge and NeoForge. |
| `axlauncher_mc_version` | [mc_version](../../apps/core/mc_version/src/lib.rs) | Pure version classification, build matching and ordering. It does not read foreign launcher data or decide how to install or launch a version. |
| `axlauncher_mod_metadata` | [mod_metadata](../../apps/core/content/mod_metadata/src/lib.rs) | Mod and loader metadata, currently the `Loader` enum. Game launch rules belong to launch. |
| `axlauncher_scheduler` | [scheduler](../../apps/core/scheduler/src/lib.rs) | Background work scheduling policy, currently priority `Band`. The originating system owns the work itself. |
| `axlauncher_observability` | [observability](../../apps/core/observability/src/lib.rs) | Command metrics and diagnostic records, currently `CommandMetric`. Telemetry owns event output. |
| `axlauncher_network` | [network](../../apps/core/network/src/lib.rs) | HTTP transport, retry classification and the shared download digest check. |
| `axlauncher_storage` | [storage](../../apps/core/storage/src/lib.rs) | Public storage entry point, filesystem primitives, platform credentials and host discovery. Document owners define schemas. |
| `axlauncher_storage_layout` | [layout](../../apps/core/storage/layout/src/lib.rs) | Storage-private implementation of directories, managed file locations and locks. Only storage may depend on it. |
| `axlauncher_state` | [state](../../apps/core/state/src/lib.rs) | Launcher settings and instance persistence, plus the in-memory `StateStore` implementation. |
| `axlauncher_accounts` | [accounts](../../apps/core/lib/accounts/src/lib.rs) | The account document, authentication and credential lifecycle. |
| `axlauncher_download_engine` | [download_engine](../../apps/core/lib/download_engine/src/lib.rs) | Execution and progress reporting for file downloads. |
| `axlauncher_java_discovery` | [java_discovery](../../apps/core/lib/java_discovery/src/lib.rs) | On-demand, read-only system Java candidate enumeration in an embedded worker. |
| `axlauncher_worker` | [worker](../../apps/core/lib/src/lib.rs) | Verified native loading, request framing, and shared process lifetime. |
| `axlauncher_install` | [install](../../apps/core/install/src/lib.rs) | Installation planning, metadata and preparation of game files before launch. |
| `axlauncher_launch` | [launch](../../apps/core/launch/src/lib.rs) | Building and executing a command from prepared files. JAR extraction uses the crate-local `NativeReader` interface. |
| `axlauncher_telemetry` | [telemetry](../../apps/core/telemetry/src/lib.rs) | Event sink implementation, currently `LoggingTelemetry`. |
| `axlauncher_core` | [core](../../apps/core/src/lib.rs) | Application-facing system re-exports and desktop request orchestration. Delegates each operation to its owning system or SPI provider. |
| `axlauncher_ui` | [ui](../../apps/ui/src/lib.rs) | Palette, theme installation and component re-exports. |
| `axlauncher_frontend` | [frontend](../../apps/frontend/src/lib.rs) | Root view, interaction state, windows and UI task lifecycle. |
| `axlauncher_modpack` | [modpack](../../apps/plugins/modpack/src/lib.rs) | Registered readers for modpack distribution inputs, plus file selection and override planning. It may depend on provider contracts, never their implementations. |
| `axlauncher_mmc_convert` | [mmc-convert](../../tools/mmc-convert/src/main.rs) | Standalone conversion of selected MultiMC exports with custom launch components into native, isolated profiles. It uses the modpack archive reader and the core facade. |
| `axlauncher_platform_windows` | [Windows](../../platforms/windows/src/main.rs) | Windows executable entry point. |
| `axlauncher_platform_linux` | [Linux](../../platforms/linux/src/main.rs) | Linux executable entry point. |
| `axlauncher_platform_macos` | [macOS](../../platforms/macos/src/main.rs) | macOS executable entry point. |
| `axlauncher_policy_tests` | [tests](../../tests/tests) | Repository policies, architecture checks and cross-crate SPI contract tests. |

## Reusing and extending interfaces

1. Check the provider contracts in the inventory and the resource owner above.
   Reuse an existing port or function when it already has the required contract.
   Keep interfaces used inside a single system local, as launch does with `NativeReader`.
2. For new behavior shared across systems, define the port in the SPI crate
   for that role, implement it
   in its owning system and inject it into consumers. Shared domain values can
   be used directly. Keep one implementation of each parser, integrity check
   or atomic write operation in its owning crate.
3. Core systems normally depend only on domain and the SPI crates they use.
   Launcher SPI uses domain and network SPI; download SPI uses network SPI.
   App-facing code accesses the contracts through `core`, without an edge to
   another provider implementation.
   State and launch may depend on storage for managed paths and atomic writes. Only
   storage may depend on its internal layout crate. The account
   and download workers use storage, network, and the shared worker protocol.
   Java discovery uses storage for system locations, launch for executable
   probes and validation, and the shared worker protocol for its only desktop
   interface. No production crate links to the Java discovery implementation.
   Install uses the SPI for planned files, storage for installation paths
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

`unsafe_code` is denied throughout the workspace. The only exceptions are the
verified image loader and the three versioned C ABI exports; a policy test
checks the exception sites. Account workers remain live for in-flight work,
a pending profile selection, or sessions that have no recoverable credential
in the system store. Otherwise the last request closes the child and releases
its scratch files. Download batches hold one worker across file requests. Java
discovery stays live only while a request is pending, without a lease.

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

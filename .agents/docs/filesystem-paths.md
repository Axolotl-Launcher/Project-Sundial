# Filesystem path policy

`axlauncher_storage` is the only public entry point for launcher path
configuration and managed locations. Its internal `axlauncher_storage_layout`
crate implements the layout. Applications call through `axlauncher_core::storage`;
other systems never depend on layout directly. Document owners still read,
validate, and write their own documents. In particular, use
`state::load_settings` and `state::update_settings` for existing preferences,
`core::desktop::accounts` for account operations, and the Java-selection APIs
in `state` for Java preferences.

## Roots and overrides

| Location | Default | API |
| --- | --- | --- |
| Launcher application data | `AXLauncher` under the system user data directory | `DataDir::path(DataPath::Root)` |
| Launcher cache | `AXLauncher` under the system cache directory | `DataDir::path(DataPath::Cache)` |
| Launcher user data (instances) | `profiles` under the application data directory | `DataDir::path(DataPath::Profiles)` |
| Launcher temporary directory | A private `AXLauncher-*` directory per operation in the system temporary directory | `DataDir::scratch(ScratchSpace::Task)` (`DataPath::Temp` selects its parent) |
| Worker library temporary directory | A private `AXLauncher-*` directory per worker under the launcher cache's `workers` directory | `DataDir::scratch(ScratchSpace::Worker)` |

`DataDir::from_environment()` resolves startup defaults once. A nonempty
`AXLAUNCHER_DATA_DIR` takes precedence over the platform defaults and keeps
the application data, cache, instances, and scratch files under one root.
`DataDir::isolated()` applies the same layout to an explicit root, without
consulting the environment. Tests and standalone tools can use this form.
When a required system location is unavailable, report the failure and let
the caller select an explicit root; do not guess a working-directory fallback.

`DataDir::set_directory(kind, Some(path))` applies an override for that
snapshot. `None` resets that directory to its default. Relative inputs are
resolved at the time of selection; empty inputs and existing non-directories
are rejected. An override does not change system settings, create or move
data, or persist across restarts. Work that has already started keeps the
snapshot it received. Callers must not rebuild one from the app data path:
that would discard other overrides.

Changing app data also changes paths derived from it, including the default
instance directory. Cache and scratch defaults follow app data only in
isolated mode; otherwise they keep their platform defaults.

The account document uses the active app data root, including in portable
packaging. The account worker scopes records to the current device and OS
principal; moving a data directory does not move that principal's saved
accounts or system credentials to another machine or user.

## Layout and filesystem operations

- Use `DataDir::game()` for the shared Minecraft tree, including the cache
  location used by both installation and native extraction. Use
  `GameDir::isolated()` for an isolated instance and `GameDir::standalone()`
  for a supplied game tree that has no shared cache. Never infer the cache
  policy from a directory's name or parent.
- Use `GameDir::version_path` and `GameDir::asset_path` for version and index
  locations. Pass a `GameDir` to the version reader so it resolves both the
  requested document and its ancestors in the same tree.
- Use `GameDir::native_classifier_archive()` for archives selected from the
  manifest's `downloads.classifiers` map. In a shared tree, a classifier
  with a usable SHA-1 lives only in the cache. Trees without a shared cache
  and classifiers without a usable hash use the manifest's Maven path.
  Do not look for a copy in another location.
- Use `Document` for writer coordination and paths of transactional documents,
  and `ManagedFile` for other fixed file locations. Acquire game-tree and
  native extraction locks through `GameDir::lock`, never by opening a lock
  file at the call site.
- Create disposable scratch space through storage. Keep atomic writes,
  download staging, and profile publication beside the destination when they
  rely on a rename. The application scratch root is not a substitute for
  same-filesystem staging.
- Extract bundled worker libraries into `DataDir::scratch(ScratchSpace::Worker)` and use
  `WorkerLibrary::path` for their filenames. Native libraries require
  executable mappings; the worker cache is independent of the temporary
  location, which may use `noexec`. Report a load failure if the cache itself
  disallows those mappings. Retain the scratch handle until the process has
  exited and the operating system has released the library.
- A user-selected file and a validated path supplied by a manifest, Maven
  coordinate, or archive are inputs, not launcher defaults. Their readers
  retain responsibility for validation. Fixed paths in a JDK, Minecraft
  save, or external archive remain part of that format; they are not
  independently configurable launcher directories.

## Enforcement

`cargo nextest run --locked -p axlauncher_policy_tests` checks production Rust
under `apps/`, `platforms/`, and `tools/`, including target-gated code and build
scripts. It rejects directory discovery outside storage and hard-coded local
paths in common construction and filesystem calls. The crate-boundary test
also restricts directory-discovery dependencies to storage. These checks run
in the workspace CI test matrix. Fixtures in `tests/` may use literal paths.

Keep exceptions local to a source file, function, and literal, and explain
why the path cannot use a storage API. The current exceptions cover JDK and
Minecraft save formats, the modpack build script's repository `.env`,
filenames private to the standalone converter's generated scratch directory,
and Linux's `/proc/self/exe` process reference for spawning embedded workers.
The check is not a dataflow analyzer; review indirect path assembly against
the same contract.

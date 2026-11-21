# AGENTS.md

Instructions for anyone — human or agent — changing this repository. Read this
before your first edit.

## What this is

A clean-room rebuild of `../Axolotl` (a Minecraft launcher) as a much smaller
Rust + gpui-kit application. It is **not** a refactor of Axolotl and shares no
code with it. When you need to know how Axolotl solved something, read it for
reference; never copy its source into this repository, and never import its
crates.

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
- **Network**: on the maintainer's machine crates.io intermittently fails TLS.
  Use the mirror config documented in `README.md` to resolve, but commit the
  resulting `Cargo.lock` unchanged.

## Do not

- Do not edit `../Axolotl`.
- Do not add CI steps that only warn where a failure would catch the problem.
- Do not reintroduce the tools this repo deliberately left out: sccache
  configuration (Axolotl declares it but never uses it), a release-cache warmer
  matrix, or `keep-a-changelog` (installed but unused there).

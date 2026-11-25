<!-- SPDX-FileCopyrightText: 2026 Axolotl Team -->
<!-- SPDX-License-Identifier: GPL-3.0-only -->

# Minecraft Java modpack formats

Sources checked on 2026-11-25. Modrinth and packwiz publish format specifications.
CurseForge documents the export layout and REST API, while the manifest fields
below are corroborated by independent reader and writer implementations.
MultiMC documents its instance export and patches, and its launcher source
defines how the component file is read. The Axolotl guidance at the end is for
future implementation.

| Format | Entry point | Where file bytes come from |
| --- | --- | --- |
| Modrinth `.mrpack` v1 | `modrinth.index.json` at the ZIP root and optional override trees | Manifest HTTPS URLs with hashes, plus bundled overrides |
| CurseForge `.zip` v1 | `manifest.json` at the ZIP root and an override tree | CurseForge files pinned by project and file ID, plus bundled overrides |
| MultiMC instance `.zip` | `instance.cfg` and `mmc-pack.json` in one instance directory, commonly inside a top-level wrapper | Bundled game files, with separate component metadata and optional local launch artifacts |
| packwiz | A `pack.toml` URL, commonly served by a static HTTP host | Indexed files on the pack host and external files pinned by their download metadata |

## Modrinth `.mrpack`

### Archive and manifest

The ZIP uses the `.mrpack` extension and media type
`application/x-modrinth-modpack+zip`. Its root `modrinth.index.json` is UTF-8.
The following fields describe version 1. [Official format specification][mr-spec]

| Field | Meaning |
| --- | --- |
| `formatVersion` | Integer `1`. |
| `game` | String `minecraft`. |
| `versionId` | Pack-defined version identifier. A catalog version ID, when available, comes from the Modrinth API. |
| `name`, `summary` | Display name and optional short description. |
| `dependencies` | Object mapping dependency IDs to version strings. `minecraft` selects the base game. The documented loader IDs are `forge`, `neoforge`, `fabric-loader`, and `quilt-loader`. |
| `files` | Array of downloadable files with paths relative to the game directory. |

Each `files` entry contains a destination `path`, a `hashes` object with both
`sha1` and `sha512`, a `downloads` array of HTTPS URLs, and an integer `fileSize`
in bytes. Additional hash keys may occur. The URLs are alternative sources for
the same bytes. Preserve path subdirectories and URL escaping. A space in a
URL must be encoded, and the spec calls for following redirects up to a
reasonable limit. The optional `env` object describes `client` and dedicated
`server` applicability independently. [Official format specification][mr-spec]

| Selected side | File selection |
| --- | --- |
| `env` absent | Include the file on either side. |
| `required` | Include the file for the selected side. |
| `optional` | Offer the file for selection on the selected side. |
| `unsupported` | Skip it on the selected side. A client's integrated server still uses the client selection. |

Copy the contents of `overrides/` into the game directory, then apply the
selected side's `client-overrides/` or `server-overrides/` over it. For example,
`client-overrides/config/example.toml` takes precedence over
`overrides/config/example.toml` on the client. The opposite side's tree is
outside that installation's file set. Resolve this precedence explicitly so
the result is independent of ZIP entry order. [Official format specification][mr-spec]

### Download and publication behavior

- The specification lists `cdn.modrinth.com`, `github.com`,
  `raw.githubusercontent.com`, and `gitlab.com` for submissions to Modrinth,
  and recommends an explicit host policy for other implementations. Apply
  the importer's network policy to the original URL and every redirect.
  [Official format specification][mr-spec]
- Modrinth's server validator checks for both hash keys. Prism's installer
  verifies SHA-512 and prompts for optional files. A local pack already carries
  enough information to download its indexed files. Modrinth's hash lookup can
  enrich catalog metadata independently. [Modrinth validator][mr-validator] ·
  [Prism installer][prism-mr] · [Modrinth hash lookup][mr-hash]
- Use the documented dependency spellings. The Modrinth App also accepts the
  historical `neo-forge` alias. Future dependency IDs can change the required
  game setup, so an unsupported ID needs an explicit diagnostic. Treat an
  absent loader as vanilla only when the game requirement is otherwise valid.
  [Official format specification][mr-spec] · [Modrinth App manifest model][mr-model]

## CurseForge ZIP

### Archive and manifest

CurseForge's submission guide shows a root `manifest.json` and `overrides/`.
The manifest's `overrides` string selects the archive-relative payload
directory. Copy its contents relative to the game directory. The payload can
include configs, scripts, saves, resource packs, and bundled files such as
approved third-party mods. CurseForge's publication rules govern which files
can be bundled for submission. An optional `modlist.html` is display material.
[Official export guide][cf-export] · [Prism manifest reader][prism-cf-read] ·
[HMCL manifest reader][hmcl-cf-read]

| Field | Meaning in the v1 interchange format |
| --- | --- |
| `manifestType`, `manifestVersion` | String `minecraftModpack` and integer `1`. Validate both before treating a generic `manifest.json` as a pack. |
| `name`, `version`, `author` | Pack display metadata. |
| `minecraft.version` | Minecraft version string. |
| `minecraft.modLoaders` | Array of `{ "id": "forge-47.4.0", "primary": true }`-shaped entries. |
| `files` | Array of `{ "projectID": 123, "fileID": 456, "required": true }`-shaped entries with integer IDs. `required: false` marks an optional file. |
| `overrides` | Relative payload directory, conventionally `overrides`. Prism defaults this field to `overrides` and defaults a missing `required` to `true`. |

Prism reads `primary` and typically exports a single loader marked primary.
Its current installer iterates recognized entries and ends up using the last
one. Treat conflicting loaders as ambiguous until Axolotl defines a selection
rule. Recognized ID prefixes include `forge-`, `fabric-`, `quilt-`, and
`neoforge-`. Split off the known prefix once to preserve the complete version
suffix. Prism handles a Minecraft `1.20.1-` prefix on NeoForge IDs for that
game version. Its writer can also emit `minecraft.recommendedRam`, an optional
launcher hint. [Prism manifest reader][prism-cf-read] ·
[Prism exporter][prism-cf-write] · [Prism installer][prism-cf-install]

### Resolving the file list

1. Resolve the exact `(projectID, fileID)` through CurseForge REST v1. Use
   `GET /v1/mods/{modId}/files/{fileId}` for a single file or
   `POST /v1/mods/files` with `fileIds` for a batch. For batch responses, check
   that returned `id` and `modId` match the manifest and account for missing
   entries. The API uses `x-api-key` authentication. [CurseForge REST API][cf-api]
2. Read `fileName`, `fileLength`, `hashes`, availability, and `downloadUrl`
   from the returned `File`. The API also exposes
   `GET /v1/mods/{modId}/files/{fileId}/download-url`. The `FileHash` algorithm
   values are `1` for SHA-1 and `2` for MD5. Use a published digest when
   supplied. The API's `fileFingerprint` is a lookup value with separate
   fingerprint endpoints. [CurseForge REST API][cf-api]
3. Derive the destination from the resolved resource type and safe file name.
   The archive manifest supplies neither a file name nor a destination path.
   Mods and resource packs commonly land in `mods/` and `resourcepacks/`
   respectively. Prism also classifies other resources and handles worlds
   separately. Keep the exact file pins when resolving the pack. An API file's
   `dependencies` describe that file's relations and do not authorize replacing
   pins or installing a project's latest release. [CurseForge REST API][cf-api] ·
   [Prism installer][prism-cf-install]
4. A missing `downloadUrl` can reflect the author's third-party distribution
   setting. Report it separately from a deleted file, invalid API key, or
   transient failure. For a blocked required entry, surface the official file
   page or a verified user-supplied copy and keep the install pending while the
   file is missing. An unselected optional entry can be skipped. Use URLs
   authorized by the provider. CurseForge announced mandatory authentication
   for direct `edge.forgecdn.net` downloads from July 16, 2026. Attach the
   key to HTTPS file requests on that origin and reject cross-origin redirects
   from keyed requests. The metadata client authenticates its own requests.
   [Distribution toggle][cf-distribution] · [CDN authentication announcement][cf-cdn] ·
   [Prism installer][prism-cf-install]

CurseForge server packs can be separate files linked through API metadata.
Decide which archive to fetch before parsing a client manifest. The v1 file
entries above encode required versus optional, with no per-side field.
[CurseForge REST API][cf-api] · [Prism manifest reader][prism-cf-read]

## MultiMC instance export

### Archive and game data

MultiMC's native export ZIP contains a selection of files from an instance.
The current exporter wraps them in a directory named after the instance. The
importer locates the instance via `instance.cfg`. Axolotl identifies a single
root or one-level wrapper directory containing both `instance.cfg` and
`mmc-pack.json`. The native export and MultiMC's separate Modrinth export
use different manifest layouts. [MultiMC export guide][mmc-guide] ·
[MultiMC exporter][mmc-export] · [MultiMC importer][mmc-import]

| Path relative to the instance directory | Role |
| --- | --- |
| `instance.cfg` | Launcher settings such as `name`, icon, play time, and version type. Settings may contain local Java paths, JVM arguments, or launch commands. |
| `mmc-pack.json` | Ordered component list and selected game and loader versions. |
| `.minecraft/` or `minecraft/` | Game data such as mods, configs, resource packs, and saves. MultiMC uses `minecraft/` when it exists and `.minecraft/` does not. |
| `patches/<uid>.json` | Local component metadata referenced by the component list. |
| `libraries/`, `jarmods/` | Optional instance-local launch artifacts used by component patches and JAR mods. |

Axolotl reads the display name from `instance.cfg`. Commands, Java and wrapper
paths, and JVM options stay in the export. Only payloads under the selected
game directory are copied to the managed profile.
[MultiMC export guide][mmc-guide] · [MultiMC instance implementation][mmc-instance]

### Components and patches

`mmc-pack.json` is a JSON object with integer `formatVersion: 1` and a
`components` array. Each entry has a `uid`. The writer emits `version` when a
version is selected. `disabled`, `important`, and `dependencyOnly` are boolean
component flags. `cachedName`, `cachedVersion`, `cachedRequires`, and
`cachedConflicts` are cached metadata. Preserve the array's component order
when reasoning about patches. The patch wiki marks its older `order` field
deprecated. [MultiMC component reader and writer][mmc-profile] ·
[MultiMC patch guide][mmc-patches]

| UID | Role |
| --- | --- |
| `net.minecraft` | Base game, normally with an explicit selected version. |
| `net.minecraftforge`, `net.neoforged` | Forge and NeoForge. |
| `net.fabricmc.fabric-loader`, `org.quiltmc.quilt-loader` | Fabric and Quilt. |
| `org.lwjgl`, `org.lwjgl3` | Runtime components, typically Minecraft dependencies. |

MultiMC resolves standard components through its metadata service. A local
`patches/<uid>.json` can change dependency constraints, libraries, the main
class, and other launch properties. It can also refer to local libraries or
JAR mods. Matching a stock UID and version alone cannot reproduce a customized
component. Axolotl accepts vanilla and one standard Forge, NeoForge, Fabric,
or Quilt loader. It resolves standard runtime dependencies through its game
and loader installers and rejects active local patches, unknown enabled
components, and instance-local launch artifacts. Older exports without
`mmc-pack.json` follow MultiMC's historical
config and patch migration path. The layout here covers the current component
file. [MultiMC metadata][mmc-meta] · [MultiMC component reader and writer][mmc-profile] ·
[MultiMC patch guide][mmc-patches]

The standalone `axlauncher-mmc-convert` tool handles supported custom
components. It applies active patches in order and produces a native version
document plus a private runtime for the new profile. It supports appended
bundled Java libraries, a bundled replacement client JAR, and ordered
bundled archive JAR mods. The desktop importer remains the entry point for
standard exports. [Converter usage](../../CONTRIBUTING.md#multimc-exports-with-custom-components)

## packwiz subscriptions

### Publishing and file resolution

The pack author maintains a directory and publishes the files it references
at an HTTP or HTTPS location. `packwiz update --all` is an authoring command
that looks for newer compatible upstream files and changes their pinned
metadata. `packwiz refresh` updates the index and its hash after local edits.
If the authoring setup uses `no-internal-hashes`, use `packwiz refresh --build`
to produce an index with hashes for distribution. `packwiz serve` refreshes
the index on request for local development. Publish the manifest, index, and
payload as a consistent snapshot. An HTTP cache serving files from different
revisions can cause a hash mismatch. A CurseForge ZIP or `.mrpack` exported
from the source directory follows the destination format's update model.
[Hosting tutorial][pw-host] · [Update command][pw-update] ·
[Refresh command][pw-refresh] · [Development server][pw-serve]

| Layer | What it supplies |
| --- | --- |
| `pack.toml` | Name, `pack-format` (currently `packwiz:1.1.0`), `[versions]` for Minecraft and loader components, and `[index]` with relative `file`, `hash-format`, and `hash`. The optional pack `version` is display metadata. |
| Index, usually `index.toml` | Default `hash-format` and `[[files]]` entries. Each entry has a relative `file` and `hash`, with optional per-entry `hash-format`, `alias`, `metafile`, and `preserve`. A regular entry supplies payload bytes at that path on the pack host. |
| Metadata, typically `*.pw.toml` | An index entry with `metafile = true` hashes this metadata. The metadata specifies a destination `filename` relative to itself, the actual payload source and digest in `[download]`, and optional `[option]` and `side` fields. The installer uses the selected physical side, with a missing side defaulting to `both`. |

The index path is relative to the pack manifest and entry paths are relative
to the index. Validate each resolved path against the pack root. The format
specifies SHA-256, SHA-512, SHA-1, MD5, and Murmur2. The reference installer
also accepts `download.mode = "metadata:curseforge"` and resolves the exact
file using the IDs in `[update.curseforge]`. A provider-blocked file can
require manual acquisition. In standard URL mode, download the published
URL and verify its digest. Other `[update]` data helps authoring tools look
up new upstream versions. [Pack manifest][pw-pack] · [Index][pw-index] ·
[File metadata][pw-file] · [Installer download mode][pw-mode] ·
[Installer CurseForge resolution][pw-cf]

### What the reference installer does

The official setup calls the installer as a launcher pre-launch task or from
a dedicated server's start script. Each invocation fetches `pack.toml` and
compares its SHA-256 with the value in the local `packwiz.json`. It can skip
the index and payload requests when the manifest is unchanged and expected
tracked files are present. It can also skip an unchanged index when only the
pack manifest changed. When needed, it verifies the index and metadata
hashes before using them, compares entry hashes with local state, and checks
payload hashes before writing. The ordinary fast path checks for missing
files without rehashing every existing file. [Hosting tutorial][pw-host] ·
[Installer update manager][pw-manager] · [Installer file tasks][pw-download]

- The installer tracks destination paths, the selected side, and optional
  choices in the local manifest. A change in side invalidates the file
  cache. It can remove tracked files that disappear from the index or become
  inapplicable. Existing files with `preserve = true` are protected from
  overwrite while their entries are indexed. The removal pass can still
  delete a previously tracked file once its entry has disappeared.
  [Index specification][pw-index] · [Installer update manager][pw-manager] ·
  [Installer file tasks][pw-download]
- `[versions]` can change independently of the index. When it finds a
  compatible `mmc-pack.json`, the installer checks for recognized game and
  loader changes and can prompt to write them into the MultiMC instance.
  The official server example uses headless mode. In the current CLI, if
  optional-file selection is invoked, it accepts all the offered options.
  [Installer launcher integration][pw-launcher] · [Installer CLI][pw-cli]
- Unreachable or invalid `pack.toml` is a fatal error in the current
  installer, including when a local manifest already exists. Files are
  applied individually, and obsolete files are removed before the download
  pass, so an update can make partial changes before failing. An installer
  error does not imply that the previous instance is intact.
  [Installer update manager][pw-manager] · [Installer file tasks][pw-download]

Verified the official example's index and all three indexed entries against
their published hashes. A local HTTP fixture with installer `v0.5.14` and
bootstrap self-update disabled also verified first install, a one-request
unchanged check, replacement when an entry's hash changes, missing-file
repair, `preserve` behavior on update and removal, digest rejection, and a
nonzero exit when the host was offline. [Official example][pw-example] ·
[Installer release][pw-release]

### Axolotl integration

Treat a packwiz URL as the source of a managed profile. After the first
install, an explicit update or an opt-in pre-launch check can compare it
with the last successfully applied revision. Store the source URL, applied
manifest and index hashes, side, optional choices, and managed file records.
Use the Axolotl loader and installation flows for `[versions]`. A MultiMC
bootstrap ZIP may advertise a packwiz URL through a pre-launch command.
Use an explicitly selected URL as the import input and avoid executing
commands from a foreign instance archive.

Compute additions, replacements, and removals before applying a revision.
Stage and verify downloads before replacing tracked files or pruning paths.
An offline or failed check can allow launch of the last complete revision
only when it is still usable. Keep user-created files outside the managed
set and define how to handle edits to tracked files before removing them.
The current download engine verifies SHA-1, while packwiz commonly uses
SHA-256 for its index and metadata and can use other algorithms for payloads.
[Crate ownership](crates.md) · [Download engine](../../apps/core/download_engine/src/lib.rs)

## Implications for Axolotl

- The registered readers expose one `Pack` preview with a source tag and
  reader-owned optional item IDs. The selected items produce one `InstallPlan`
  for the modpack service to execute. The plan carries game requirements, downloads,
  bundled files, and a redirect policy. The frontend uses the source tag for
  presentation. Modrinth, CurseForge, and MultiMC have registered readers.
  CurseForge planning resolves the selected file pins through its API. The
  import preview enables official CDN URL derivation by default for files
  whose metadata omits a direct URL. When automatic acquisition is unavailable
  or fails, the user can supply a file from the official download page and the
  installer verifies it against the pinned digest. Continuing the same
  import reuses its resolved plan, staging directory, and successful results.
  Root-level `datapacks/` is a storage location.
  Vanilla Minecraft loads data packs from a world or through the new-world
  Data Packs screen. [CurseForge data pack guide][cf-datapacks]
  [Plugin entry point](../../apps/plugins/modpack/src/lib.rs)
- The modpack service prepares pack files and the game concurrently after
  planning. The pack batch writes to private staging, and the game installer
  holds the existing shared game-root lock. Each batch applies the configured
  file concurrency independently and uses the shared download engine. The
  service publishes after both branches succeed. Import and Download debug
  display the same task progress. Download debug samples the engine's global
  transfer statistics while an import is running.
  [Modpack service](../../apps/frontend/src/modpacks/service.rs) ·
  [Download debug](../../apps/frontend/src/panel/download_debug.rs)
- Keep Modrinth, CurseForge, and MultiMC exports as distribution inputs to a
  new profile. MultiMC exports contribute bundled files and game requirements
  to the same plan. Converting installed foreign launcher data belongs in an
  independent tool. The managed instance schema remains `.axlprofile` and its
  creation, reads, and discovery belong to `state`.
  [Repository rule](../../AGENTS.md) · [Crate ownership](crates.md)
- Identify the format from the manifest shape and its version. For the provider
  formats, use their documented ZIP roots. Reject ambiguous or duplicate
  manifest candidates and keep nested payload manifests out of detection.
  Locate the MultiMC wrapper inside its reader.
- Resolve the exact Minecraft and loader build through the existing
  `ModLoader` port and installation flows. Use the injected `Network` and the
  download engine for transfers. The current engine's `MissingFile` carries a
  primary URL and SHA-1 fields. Its digest path verifies SHA-1, so full
  `.mrpack` integrity checks need a way to validate SHA-512 before publishing
  the profile. Use the Modrinth `fileSize` as a scheduling hint and validate
  the downloaded bytes against both manifest digests before publication.
  [Crate ownership](crates.md) · [Download engine](../../apps/core/download_engine/src/lib.rs)
- Plan selected files before writing. Validate archive entry paths, manifest
  paths, and API-derived file names against the destination root. Account for
  absolute paths, traversal, links, Windows path rules, case collisions, and
  file-directory conflicts. Apply decompressed size and count limits. Resolve
  documented override collisions separately from duplicate destinations in
  one layer. Stage new installs so failure or cancellation leaves existing
  data intact and does not publish a partially prepared profile.
- Verify with small fixtures that exercise the format-specific behavior. Cover
  Modrinth side filters, optional selection, alternative URLs, digest failure,
  and side-specific override precedence. Cover CurseForge pair matching,
  missing download URLs, and non-mod destinations. For MultiMC exports,
  cover the wrapper, both game-directory names, disabled components, and an
  unsupported local patch.

PCL CE confirms the common and client-only override order and provides an
example of selecting MultiMC patches. Its patch reader sorts by the patch
`order` field, while MultiMC's modern component list records the order. The
earlier Axolotl code also explores archive base folders and override collisions.
Use these implementations to locate edge cases and defer to the primary sources
for format behavior. [PCL CE Modrinth reader][pcl-mr] · [PCL CE MultiMC reader][pcl-mmc] ·
[MultiMC component reader and writer][mmc-profile]

[mr-spec]: https://support.modrinth.com/en/articles/8802351-modrinth-modpack-format-mrpack
[mr-validator]: https://github.com/modrinth/code/blob/a34b3935795cd46baa23ec427db4ebb0718210bb/apps/labrinth/src/validate/modpack.rs
[mr-model]: https://github.com/modrinth/code/blob/a34b3935795cd46baa23ec427db4ebb0718210bb/packages/app-lib/src/api/pack/install_from.rs#L27-L90
[mr-hash]: https://docs.modrinth.com/api/operations/versionfromhash/
[prism-mr]: https://github.com/PrismLauncher/PrismLauncher/blob/768ddbb61f78841cc0913055a6d87f179fd8b70f/launcher/modplatform/modrinth/ModrinthInstanceCreationTask.cpp
[cf-export]: https://support.curseforge.com/support/solutions/articles/9000197908-exporting-a-modpack-for-curseforge-project-submission
[cf-api]: https://docs.curseforge.com/rest-api/
[cf-datapacks]: https://support.curseforge.com/support/solutions/articles/9000267956
[cf-distribution]: https://support.curseforge.com/en/support/solutions/articles/9000207877
[cf-cdn]: https://blog.curseforge.com/introducing-api-key-authentication-for-curseforge-file-downloads/
[prism-cf-read]: https://github.com/PrismLauncher/PrismLauncher/blob/768ddbb61f78841cc0913055a6d87f179fd8b70f/launcher/modplatform/flame/PackManifest.cpp
[prism-cf-write]: https://github.com/PrismLauncher/PrismLauncher/blob/768ddbb61f78841cc0913055a6d87f179fd8b70f/launcher/modplatform/flame/FlamePackExportTask.cpp#L373-L435
[prism-cf-install]: https://github.com/PrismLauncher/PrismLauncher/blob/768ddbb61f78841cc0913055a6d87f179fd8b70f/launcher/modplatform/flame/FlameInstanceCreationTask.cpp
[hmcl-cf-read]: https://github.com/HMCL-dev/HMCL/blob/6f3c7fdee1849d6578b7635158d2623eea89e95e/HMCLCore/src/main/java/org/jackhuang/hmcl/modpack/curse/CurseModpackProvider.java#L51-L64
[mmc-guide]: https://github.com/MultiMC/Launcher/wiki/Export-Instance
[mmc-export]: https://github.com/MultiMC/Launcher/blob/b56ab2ebed03c8e33ed8829b3fc1d24b8df332b5/launcher/ui/dialogs/ExportInstanceDialog.cpp#L379-L410
[mmc-import]: https://github.com/MultiMC/Launcher/blob/b56ab2ebed03c8e33ed8829b3fc1d24b8df332b5/launcher/InstanceImportTask.cpp#L105-L161
[mmc-instance]: https://github.com/MultiMC/Launcher/blob/b56ab2ebed03c8e33ed8829b3fc1d24b8df332b5/launcher/minecraft/MinecraftInstance.cpp
[mmc-profile]: https://github.com/MultiMC/Launcher/blob/b56ab2ebed03c8e33ed8829b3fc1d24b8df332b5/launcher/minecraft/PackProfile.cpp
[mmc-patches]: https://github.com/MultiMC/Launcher/wiki/JSON-Patches
[mmc-meta]: https://github.com/MultiMC/meta-multimc/tree/f79b1bbaa1ab94d79643cc54dd1c9b8ded70321a
[pcl-mr]: https://github.com/PCL-Community/PCL2-CE/blob/030bbddb3f7320286cc11c2e6e61370821ac7a9e/PCL.Launcher.Core/Modpack/Modrinth.cs
[pcl-mmc]: https://github.com/PCL-Community/PCL2-CE/blob/030bbddb3f7320286cc11c2e6e61370821ac7a9e/PCL.Launcher.Core/Modpack/MultiMC.cs
[pw-host]: https://packwiz.infra.link/tutorials/installing/packwiz-installer/
[pw-update]: https://packwiz.infra.link/reference/commands/packwiz/update/
[pw-refresh]: https://packwiz.infra.link/reference/commands/packwiz/refresh/
[pw-serve]: https://packwiz.infra.link/reference/commands/packwiz/serve/
[pw-pack]: https://packwiz.infra.link/reference/pack-format/pack-toml/
[pw-index]: https://packwiz.infra.link/reference/pack-format/index-toml/
[pw-file]: https://packwiz.infra.link/reference/pack-format/mod-toml/
[pw-mode]: https://github.com/packwiz/packwiz-installer/blob/7420866dfc6ae1f68079a166e9804b7ec31a59ca/src/main/kotlin/link/infra/packwiz/installer/metadata/DownloadMode.kt
[pw-cf]: https://github.com/packwiz/packwiz-installer/blob/7420866dfc6ae1f68079a166e9804b7ec31a59ca/src/main/kotlin/link/infra/packwiz/installer/metadata/curseforge/CurseForgeSourcer.kt
[pw-manager]: https://github.com/packwiz/packwiz-installer/blob/7420866dfc6ae1f68079a166e9804b7ec31a59ca/src/main/kotlin/link/infra/packwiz/installer/UpdateManager.kt
[pw-download]: https://github.com/packwiz/packwiz-installer/blob/7420866dfc6ae1f68079a166e9804b7ec31a59ca/src/main/kotlin/link/infra/packwiz/installer/DownloadTask.kt
[pw-launcher]: https://github.com/packwiz/packwiz-installer/blob/7420866dfc6ae1f68079a166e9804b7ec31a59ca/src/main/kotlin/link/infra/packwiz/installer/LauncherUtils.kt
[pw-cli]: https://github.com/packwiz/packwiz-installer/blob/7420866dfc6ae1f68079a166e9804b7ec31a59ca/src/main/kotlin/link/infra/packwiz/installer/ui/cli/CLIHandler.kt
[pw-example]: https://github.com/packwiz/packwiz-example-pack/tree/313b70b6be4ef05194c1f6898b42da1dbabcfb80
[pw-release]: https://github.com/packwiz/packwiz-installer/releases/tag/v0.5.14

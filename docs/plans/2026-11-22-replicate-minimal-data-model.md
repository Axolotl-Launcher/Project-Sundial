<!-- SPDX-FileCopyrightText: 2026 The Superlogical Authors -->
<!-- SPDX-License-Identifier: GPL-3.0-only -->

# Replicating the reference's data model: the minimal set

> Scope: eight of the reference's 108 tables, mapped onto this project's
> versioned JSON documents instead of SQLite.
> Reference: Axolotl `v1.11.2-beta.2`, `packages/app-lib/migrations/`.

## What the reference actually has

154 migrations producing 108 tables. The domains are uneven: 27 `instance_*`,
14 `resource*`, 8 `synced*`, 8 `content_*`, then a long tail of AI providers,
translation caches, screenshot grouping and telemetry.

Reproducing all of it would import a decade of accretion. The minimal set is
the part our verified launch path actually reads.

## The eight tables, and what each is for

| Reference table | Why it is in the minimal set |
|---|---|
| `instances` | Identity, path, name, icon, timestamps, playtime |
| `instance_links` | Which external project a instance came from |
| `instance_files` | Per-file index: relative path, sha1, size, missing flag |
| `settings` | Single-row document, `id = 0` check constraint |
| `java_versions` | The user's chosen runtime per major version |
| `discovered_javas` | Cache of found runtimes, keyed by path |
| `install_jobs` | Job status plus a JSONB `state` |
| `install_job_items` | Per-item progress within a job |

Two of them carry a lesson worth keeping: `settings` is a single row enforced
by `CHECK (id = 0)`, and `install_jobs.state` is an opaque JSONB blob so a job's
internals never enter the schema.

## Shape on our side

A JSON document per domain, each inside the envelope already used by
`settings.json` and `instance.json`:

```
data/
  instances/<id>/instance.json     # was: instances row (+ instance_links)
  instances/<id>/files.json        # was: instance_files rows
  settings.json                    # was: settings row
  javas.json                       # was: java_versions + discovered_javas
  jobs/<id>.json                   # was: install_jobs row (+ items inline)
```

### `instances` → `instance.json`

Reference columns mapped, with what we drop and why:

| Column | Ours | Note |
|---|---|---|
| `id`, `path`, `name`, `icon_path` | keep | `path` becomes the directory name's parent |
| `created`, `modified`, `last_played` | keep | Unix milliseconds, as the reference stores them |
| `submitted_time_played`, `recent_time_played` | keep | Two counters, not one |
| `install_stage` | keep | Needed to tell a half-installed instance from a ready one |
| `update_channel`, `launcher_feature_version` | **drop** | Versioning our own schema replaces both |
| `applied_content_set_id` | **drop** | References content sets, outside this set |

`instance_links` folds in as an optional `link` object, since it is 1:1 with
the row and we have no join to perform. Only `link_kind` and the Modrinth ids
are kept; the server, hosting and shared-instance columns belong to features
this project does not have.

### `instance_files` → `files.json`

| Column | Ours | Note |
|---|---|---|
| `relative_path`, `file_name`, `enabled` | keep | `file_name` is derivable but the reference stores it; drop it and derive |
| `sha1`, `size`, `missing` | keep | `missing` is what makes a launch decide to re-download |
| `id` | **drop** | `(instance, relative_path)` is the real key, as the reference's UNIQUE constraint says |
| `added_at`, `modified_at` | keep | |

The file collection is a map keyed by `relative_path` rather than an array: the
reference's uniqueness constraint is the real key, and a map enforces it.

### `settings` → `settings.json`

Ours already exists with `theme`. This adds the fields the reference's row has
and we will use: `max_concurrent_downloads`, `max_concurrent_writes`,
`default_page`, `telemetry`, and per-source download mirrors. Everything else
in the reference's 60-plus columns stays out until a feature needs it.

### `java_versions` + `discovered_javas` → `javas.json`

Two documents in one file: `chosen` (major → path) and `discovered` (path →
details). The reference keeps them apart because one is user intent and the
other is a cache; keeping that distinction inside one file preserves it
without a second read.

`discovered_javas.file_size` and `file_mtime_ms` are the cache's validity
check — keep both.

### `install_jobs` + `install_job_items` → `jobs/<id>.json`

`status` stays a field; `state` stays an opaque JSON object, exactly as the
reference keeps it, because a job's internals change more often than its
envelope. Items nest inside the job: they are never queried apart from it.

## Order of work

1. `state::files` — the per-instance file index, with its map key and the
   `missing` flag. Tests: round-trip, key uniqueness, a missing entry, a
   corrupt document.
2. `state::instances` — extend the existing manifest with the kept columns and
   the optional `link`. Tests: old documents still parse (the new fields are
   optional), and the dropped fields are absent.
3. `state::javas` — chosen and discovered, with the size/mtime cache check.
4. `state::jobs` — a job document with an opaque state and inline items.
5. `settings` — add the concurrency and mirror fields.

Each is a document with `schema_version`, using the same refusal rules as the
existing ones: a newer schema is read-only, never migrated.

## What the full-range run found (added after the first implementation)

The data model alone does not carry a version from download to launch. Running
the chain against real data — 1.5.2, 1.6.4, 1.21.11, and a real NeoForge
1.20.2 profile — surfaced three things that had to be built or corrected
alongside it. None of them is a data-model question, but all of them are part
of "下载、补丁、校验、启动" and would have been discovered one crash at a time:

1. **Java selection follows the document, not the environment.** 26.3 states
   `javaVersion.majorVersion: 25`; launching it on the Java 21 that was first
   on `PATH` failed with `UnsupportedClassVersionError` (class file 69 vs 65).
   `SystemJava::executable_for` now answers only for a runtime whose `release`
   file reports the required major. A caller's explicit preference is still
   honoured as given, so a deliberate choice is never silently overridden.

2. **A native classifier jar is a classpath entry as well as an extraction
   source.** Excluding it from the classpath started 26.3 and killed it inside
   LWJGL with `Failed to locate library: lwjgl.dll`. `docs/game-format-changes.md`
   §5 already stated the rule; the classpath builder contradicted it.

3. **Loader profiles need a patch stage, not just inheritance.** NeoForge's
   client jar does not exist until its `processors` run. The runner resolves
   `{NAME}` from `data` plus five launcher-supplied values (`SIDE`,
   `MINECRAFT_JAR`, `MINECRAFT_VERSION`, `ROOT`, `LIBRARY_DIR`), `[coord]` to a
   library path, and skips a step whose outputs are current — including the
   `binarypatcher` case, whose output is only named on its own command line.

Verified end to end on this machine: 1.21.11 (4670 files, all SHA-1 checked,
4593 assets), 1.6.4 (`virtual/legacy` tree, 1120 files), 1.5.2 (`resources/`
tree, 749 files), NeoForge 1.20.2 (six client processors, producing
`neoforge-20.2.93-client.jar`), and 26.3 actually reaching the game window with
LWJGL 3.4.3 and its sound engine started.

## What this deliberately does not do

- No SQLite. The reference uses it to join and index 108 tables; we have eight
  documents and no joins.
- No content sets, resources, synchronisation, telemetry events, AI settings,
  translation caches or screenshot grouping. Those are separate features with
  their own data, not part of "list an instance and launch it".
- No compatibility with the reference's database itself. Reading an Axolotl
  install's SQLite file is a conversion, which belongs in an external tool.

## Open questions

1. `instance_files` for a large modpack is thousands of rows; a single JSON
   document loads all of them. Acceptable until a real pack is measured, but
   the reference indexes `sha1` and `missing` for a reason.
2. The reference stores timestamps as Unix milliseconds. We should match,
   rather than inventing a format, so an external converter is a direct map.

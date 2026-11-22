<!-- SPDX-FileCopyrightText: 2026 Axolotl Team -->
<!-- SPDX-License-Identifier: GPL-3.0-only -->

# GitNexus for Axolotl

Run commands from the repository root. MCP clients use the repository name
`Axolotl-2.0`. The CLI examples use `--repo .` to select the current checkout.

## First checkout and index refresh

The index and its CLI runner live in the gitignored `.gitnexus/` directory.
On a fresh clone, bootstrap them with the repository's package manager:

```sh
pnpm --allow-build=@ladybugdb/core --allow-build=gitnexus --allow-build=tree-sitter dlx gitnexus@1.6.12 analyze --index-only
```

For an existing checkout, check freshness and refresh a stale index:

```sh
node .gitnexus/run.cjs status
node .gitnexus/run.cjs analyze --index-only
```

Use `--index-only` for both operations that build an index. It preserves the
handwritten [root instructions](../../AGENTS.md) and skips skill generation.

## Navigation and impact

| Question | MCP tool | CLI example |
| --- | --- | --- |
| Which execution flows implement this concept? | `query({search_query: "prepare"})` | `node .gitnexus/run.cjs query prepare --repo .` |
| Who calls this symbol and what does it call? | `context({name: "discover"})` | `node .gitnexus/run.cjs context discover --repo .` |
| What depends on the target I will edit? | `impact({target: "discover", direction: "upstream"})` | `node .gitnexus/run.cjs impact discover --direction upstream --repo .` |

Start with the graph for callers, imports, dependencies and execution flows.
For ambiguous names, select the returned UID with `--uid`, or qualify with
`--file`. Before editing, report the direct callers, affected processes and
risk. A file result can omit process and module axes. Check the symbol result
for a change to a function, class or method.

- **HIGH or CRITICAL** — warn before editing and identify the affected callers
  and processes. Use the reported `risk` even if `riskSharedAxes` is lower.
- **UNKNOWN or empty** — the graph could not resolve callers. Search the
  symbol or literal with `rg` and inspect the matching call sites before
  deciding whether to change or delete it.
- **Renames** — use the GitNexus MCP `rename` operation so references follow
  the symbol. Review its proposed edits.

MCP resources under `gitnexus://repo/Axolotl-2.0/` include `context`,
`clusters`, `processes` and `process/{name}`. For a security review,
`explain({target: "fileOrSymbol"})` reports source-to-sink flows when the
index was built with `analyze --index-only --pdg`.

## Review changes

Before committing, run `detect_changes({scope: "all"})` or the first command
below. Use the second to review changes relative to the target branch.

```sh
node .gitnexus/run.cjs detect-changes --scope all --repo .
node .gitnexus/run.cjs detect-changes --scope compare --base-ref main --repo .
```

If the result has `partial: true` or `truncated: true`, rerun the analysis
and resolve the missing coverage. A zero in an incomplete result cannot
establish that a change has no impact.

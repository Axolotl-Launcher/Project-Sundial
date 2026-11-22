<!-- SPDX-FileCopyrightText: 2026 The Superlogical Authors -->
<!-- SPDX-License-Identifier: GPL-3.0-only -->

# The game's format changes a launcher must absorb

Minecraft: Java Edition has changed its on-disk and manifest formats several
times. Each change is **additive in the launcher's code**, not a replacement:
a third-party launcher must serve every version a user still has, so it keeps
both the old and the new path and branches on the manifest's shape rather than
on a version number.

Every claim below was verified against the reference launcher's source, and the
manifest facts against Mojang's own `piston-meta` documents. The lesson that
matters for this project: **branch on what the manifest says, never on a parsed
version string.**

## 1. 1.6: the layout players see

`versions/<id>/<id>.jar`, Maven-path `libraries/`, hashed `assets/`. The old
`bin/` directory has no launch path in the reference. `launcher_profiles.json`
is not read to launch — only for importing official-launcher profiles.

`resources/` still matters, but as a **derived** flat tree rebuilt from hashed
objects, for versions whose game reads plain asset paths.

## 2. 1.7.3: assets become content-addressed

Objects live at `assets/objects/<first two sha1 chars>/<sha1>`; the mapping is
in `assets/indexes/<index>.json`. Older versions read a flat tree instead.

The reference gates on `version.assets == "legacy"` — a **string in the
manifest**, not a version cutoff — and rebuilds the flat tree for it. It does
not recognise `assets: "pre-1.6"` (1.5.2 and older), which is a gap there.

## 3. 1.13: arguments become structured and rule-gated

The flat `minecraftArguments` string gave way to `arguments.game` and
`arguments.jvm`, each entry either a bare string or `{ rules, value }`.

The reference keeps **both**: it emits the legacy string when present and then
the modern array, dropping legacy options already covered by the modern list to
avoid a duplicate `--gameDir`. When a manifest has no `arguments.jvm`, it
falls back to a hard-coded `-Djava.library.path=… -cp …`.

## 4. 1.17: the runtime directory and the Store profile rename

Java is chosen from the manifest's `javaVersion.majorVersion`, defaulting to
**8** when absent — never from the version id. The official launcher's runtimes
sit at `runtime/<component>/<platform>/<component>/`, with a macOS bundle
variant, and are read for discovery when reusing them.

`launcher_profiles_microsoft_store.json` appears nowhere in the reference: it is
a Store-launcher detail that no launch path depends on.

## 5. 1.19: split natives — the change that is easiest to get wrong

Native JARs are now declared as ordinary four-part Maven coordinates
(`…:natives-windows`) with `downloads.artifact`, rather than a `natives` map
plus `downloads.classifiers`.

**Both still happen, and both are required:**

- the classifier JAR goes **on the classpath**;
- it is also **extracted** into a natives directory, and
  `-Djava.library.path=${natives_directory}` is still present in modern
  manifests (verified in 1.19.2 and 26.1 documents).

So "the extraction was replaced" is wrong: it was added to. A launcher that
only classpaths breaks pre-1.19; one that only extracts never downloads the
modern native JARs, because they are not declared in the old shape.

The reference picks the classifier without a version test, and caches modern
natives by SHA-1 rather than substituting the Maven artifact — a substitution
could pair a different classifier with the same base name and produce an
ABI-incompatible set of libraries.

## 6. 26.x: year-based versions and Java 25

Launch logic never parses the id as `1.x`; it reads the manifest. Java 25 maps
to the `java-runtime-epsilon` component, and 25+ needs the JEP 512
`--add-opens=java.base/jdk.internal.misc=ALL-UNNAMED`. The removal of
obfuscation needs no launcher branch at all: launch reads `mainClass`,
libraries and arguments.

## What this means for this project

| Decision | Why |
|---|---|
| Read `javaVersion.majorVersion`, default 8 | Old manifests have no field; parsing ids as `1.x` breaks 26.x |
| Branch on manifest shape, not version | The formats overlap, and a version comparison would have to reproduce Mojang's release history exactly |
| Keep both natives paths | Modern manifests still name `${natives_directory}`; old ones declare natives only through `classifiers` |
| Rebuild the flat asset tree for `legacy` | The 1.7.2-and-older game reads plain paths |
| Support flat `minecraftArguments` | A manifest without `arguments` otherwise produces no game arguments at all |

Whether to support versions **older than 1.19** is a product decision, not a
technical one: it changes how much of the above is required.

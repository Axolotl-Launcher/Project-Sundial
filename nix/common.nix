# SPDX-FileCopyrightText: 2026 Axolotl Team
# SPDX-License-Identifier: GPL-3.0-only

# What every artifact shares: the cargo inputs, the workspace metadata, the
# glibc floor and the build steps for the embedded worker libraries.

{ lib }:
let
  src = lib.fileset.toSource {
    root = ../.;
    fileset = lib.fileset.unions [
      ../Cargo.lock
      ../.cargo/config.toml
      # apps/ui embeds the window icon with include_bytes!.
      ../packaging/icons
      ../apps/ui/content-render/assets
      (lib.fileset.fileFilter (file: file.name == "Cargo.toml") ../.)
      (lib.fileset.fileFilter (file: file.hasExt "rs") ../.)
    ];
  };

  # Read the workspace manifest from the tree, not from the filtered source:
  # the filtered path is a derivation, and reading it would make evaluation
  # depend on building it, which `nix flake check --no-build` refuses.
  manifest = lib.importTOML ../Cargo.toml;
  smokeWorker = ''
    smoke_worker() {
      if printf '\012\000\000\000"Shutdown"' | "$1" --axlauncher-worker "$2" "$3" > worker-ready.frame &&
          printf '\007\000\000\000"Ready"' | cmp - worker-ready.frame; then
        return 0
      fi
      echo "The $2 worker did not return the expected Ready frame." >&2
      od -An -tx1 worker-ready.frame >&2
      return 1
    }
  '';
in
{
  inherit src smokeWorker;

  version = manifest.workspace.package.version;

  meta = {
    description = "Axolotl Launcher, a Minecraft launcher";
    homepage = manifest.workspace.package.repository;
    license = lib.licenses.gpl3Only;
    mainProgram = "axlauncher";
    platforms = [ "x86_64-linux" ];
  };

  # Symbols baked at link time decide the floor, so this is the oldest glibc the
  # legacy build may be linked against.
  glibcFloor = "2.35";

  # The platform binary embeds the worker libraries with include_bytes!, so
  # their bytes are final before it links.
  buildPhase =
    {
      # Strip the store rpath from the worker images: a worker runs under the
      # target loader.
      patchImages,
      # The legacy dependency stage compiles a dummy launcher, so it cannot
      # execute the worker handshake. The real artifacts run it where supported.
      smoke ? true,
    }:
    ''
      runHook preBuild

      cargo rustc --locked --offline --release -p axlauncher_download_engine \
        --lib --crate-type cdylib --features worker
      cargo rustc --locked --offline --release -p axlauncher_accounts \
        --lib --crate-type cdylib
      cargo rustc --locked --offline --release -p axlauncher_java_discovery \
        --lib --crate-type cdylib
      cargo rustc --locked --offline --release -p axlauncher_instance_settings \
        --lib --crate-type cdylib
      cargo rustc --locked --offline --release -p axlauncher_install_runtime \
        --lib --crate-type cdylib
      cargo rustc --locked --offline --release -p axlauncher_modrinth \
        --lib --crate-type cdylib

      ${lib.optionalString patchImages ''
        patchelf --set-rpath "" target/release/libaxlauncher_download_engine.so
        patchelf --set-rpath "" target/release/libaxlauncher_accounts.so
        patchelf --set-rpath "" target/release/libaxlauncher_java_discovery.so
        patchelf --set-rpath "" target/release/libaxlauncher_instance_settings.so
        patchelf --set-rpath "" target/release/libaxlauncher_modrinth.so
      ''}
      patchelf --set-rpath "" target/release/libaxlauncher_install_runtime.so

      cp "$PWD/target/release/libaxlauncher_download_engine.so" "$PWD/libdownload.so"
      cp "$PWD/target/release/libaxlauncher_accounts.so" "$PWD/libaccounts.so"
      cp "$PWD/target/release/libaxlauncher_java_discovery.so" "$PWD/libjava-discovery.so"
      cp "$PWD/target/release/libaxlauncher_instance_settings.so" "$PWD/libinstance-settings.so"
      cp "$PWD/target/release/libaxlauncher_install_runtime.so" "$PWD/libinstall.so"
      cp "$PWD/target/release/libaxlauncher_modrinth.so" "$PWD/libmodrinth.so"

      export AXLAUNCHER_DOWNLOAD_IMAGE="$PWD/libdownload.so"
      export AXLAUNCHER_ACCOUNT_IMAGE="$PWD/libaccounts.so"
      export AXLAUNCHER_JAVA_DISCOVERY_IMAGE="$PWD/libjava-discovery.so"
      export AXLAUNCHER_INSTANCE_SETTINGS_IMAGE="$PWD/libinstance-settings.so"
      export AXLAUNCHER_INSTALL_IMAGE="$PWD/libinstall.so"
      export AXLAUNCHER_MODRINTH_IMAGE="$PWD/libmodrinth.so"

      # include_bytes! does not fingerprint the image contents, so a cached
      # platform binary can retain an older worker image. Invalidate the
      # platform output before relinking.
      touch platforms/entry.rs
      rm -f target/release/axlauncher
      cargo build --locked --offline --release -p axlauncher_platform_linux --bin axlauncher

      ${lib.optionalString smoke ''
        ${smokeWorker}
        smoke_worker "$PWD/target/release/axlauncher" install "$PWD/libinstall.so"
        smoke_worker "$PWD/target/release/axlauncher" modrinth "$PWD/libmodrinth.so"
      ''}

      runHook postBuild
    '';

  installPhase = ''
    runHook preInstall

    install -Dm755 target/release/axlauncher "$out/bin/axlauncher"

    runHook postInstall
  '';
}

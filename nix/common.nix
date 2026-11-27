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
      (lib.fileset.fileFilter (file: file.name == "Cargo.toml") ../.)
      (lib.fileset.fileFilter (file: file.hasExt "rs") ../.)
    ];
  };

  # Read the workspace manifest from the tree, not from the filtered source:
  # the filtered path is a derivation, and reading it would make evaluation
  # depend on building it, which `nix flake check --no-build` refuses.
  manifest = lib.importTOML ../Cargo.toml;
  smokeInstall = ''
    smoke_install() {
      printf '\012\000\000\000"Shutdown"' | "$1" --axlauncher-worker install "$2" > install-ready.frame
      printf '\007\000\000\000"Ready"' | cmp - install-ready.frame
    }
  '';
in
{
  inherit src smokeInstall;

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

      ${lib.optionalString patchImages ''
        patchelf --set-rpath "" target/release/libaxlauncher_download_engine.so
        patchelf --set-rpath "" target/release/libaxlauncher_accounts.so
        patchelf --set-rpath "" target/release/libaxlauncher_java_discovery.so
        patchelf --set-rpath "" target/release/libaxlauncher_instance_settings.so
      ''}
      patchelf --set-rpath "" target/release/libaxlauncher_install_runtime.so

      export AXLAUNCHER_DOWNLOAD_IMAGE="$PWD/target/release/libaxlauncher_download_engine.so"
      export AXLAUNCHER_ACCOUNT_IMAGE="$PWD/target/release/libaxlauncher_accounts.so"
      export AXLAUNCHER_JAVA_DISCOVERY_IMAGE="$PWD/target/release/libaxlauncher_java_discovery.so"
      export AXLAUNCHER_INSTANCE_SETTINGS_IMAGE="$PWD/target/release/libaxlauncher_instance_settings.so"
      export AXLAUNCHER_INSTALL_IMAGE="$PWD/target/release/libaxlauncher_install_runtime.so"

      cargo build --locked --offline --release -p axlauncher_platform_linux --bin axlauncher

      cp "$AXLAUNCHER_INSTALL_IMAGE" "$PWD/libinstall.so"
      ${smokeInstall}
      smoke_install "$PWD/target/release/axlauncher" "$PWD/libinstall.so"

      runHook postBuild
    '';

  installPhase = ''
    runHook preInstall

    install -Dm755 target/release/axlauncher "$out/bin/axlauncher"

    runHook postInstall
  '';
}

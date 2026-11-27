# SPDX-FileCopyrightText: 2026 Axolotl Team
# SPDX-License-Identifier: GPL-3.0-only

# What every artifact shares: the cargo inputs, the workspace metadata, the
# glibc floor and the build steps for the two embedded worker libraries.

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

  manifest = lib.importTOML "${src}/Cargo.toml";
in
{
  inherit src;

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

  # The platform binary embeds both worker libraries with include_bytes!, so
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

      ${lib.optionalString patchImages ''
        patchelf --set-rpath "" target/release/libaxlauncher_download_engine.so
        patchelf --set-rpath "" target/release/libaxlauncher_accounts.so
      ''}

      export AXLAUNCHER_DOWNLOAD_IMAGE="$PWD/target/release/libaxlauncher_download_engine.so"
      export AXLAUNCHER_ACCOUNT_IMAGE="$PWD/target/release/libaxlauncher_accounts.so"

      cargo build --locked --offline --release -p axlauncher_platform_linux --bin axlauncher

      runHook postBuild
    '';

  installPhase = ''
    runHook preInstall

    install -Dm755 target/release/axlauncher "$out/bin/axlauncher"

    runHook postInstall
  '';
}

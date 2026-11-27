# SPDX-FileCopyrightText: 2026 Axolotl Team
# SPDX-License-Identifier: GPL-3.0-only

# The binary for nix users: linked against nixpkgs' glibc, with a store rpath
# plus the directories whose libraries it dlopens.

{
  craneLib,
  common,
  lib,
  pkgs,
}:
let
  inherit (common) src version meta;

  args = {
    pname = "axlauncher";
    inherit version src meta;
    strictDeps = true;
    # aws-lc-sys, reached through reqwest's rustls feature, builds C and needs
    # cmake and perl.
    nativeBuildInputs = with pkgs; [
      cmake
      patchelf
      perl
      pkg-config
    ];
    # vulkan-loader and wayland are never linked: ash and wayland-sys dlopen
    # them, so their store paths only feed the rpath below.
    buildInputs = with pkgs; [
      fontconfig
      freetype
      libx11
      libxcb
      libxkbcommon
      vulkan-loader
      wayland
    ];
    cargoLock = ../Cargo.lock;
    env.CARGO_NET_OFFLINE = "true";
    doCheck = false;
    # installPhase handles the artifacts itself; crane's build-log hook
    # would otherwise expect `cargo build` output on stdout.
    doNotPostBuildInstallCargoBinaries = true;
    # The final rpath is set after the link, where the fixup would otherwise
    # shrink it away.
    dontPatchELF = true;
  };

  # NixOS keeps the host graphics driver outside the store.
  storeRpath = lib.concatStringsSep ":" [
    "${pkgs.wayland}/lib"
    "${pkgs.vulkan-loader}/lib"
    "/run/opengl-driver/lib"
  ];
  # Exposed so CI can cache the expensive half of the build on its own.
  deps = craneLib.buildDepsOnly args;
in
{
  inherit deps;

  binary = craneLib.buildPackage (
    args
    // {
      cargoArtifacts = deps;
      buildPhase = common.buildPhase { patchImages = false; };
      outputs = [
        "out"
        "workerProbe"
        "installImage"
      ];
      installPhase = common.installPhase;
      postFixup = ''
        patchelf --add-rpath '${storeRpath}' "$out/bin/axlauncher"
        install -Dm644 libinstall.so "$installImage/libinstall.so"
        # Install the probe after fixup so it keeps the exact bytes embedded
        # in the platform binary.
        install -Dm644 libjava-discovery.so \
          "$workerProbe/java-discovery-image"
      '';
    }
  );
}

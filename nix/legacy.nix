# SPDX-FileCopyrightText: 2026 Axolotl Team
# SPDX-License-Identifier: GPL-3.0-only

# The binary for traditional distributions: linked against the glibc of the
# oldest supported release, with a system interpreter and no store reference.

{
  common,
  craneLib,
  lib,
  pkgs,
  pkgsOld,
  toolchain,
}:
let
  inherit (common) src version meta;

  # crane cannot be instantiated on the 22.11 package set, so these are plain
  # derivations: the old stdenv links, the current toolchain compiles, and crane
  # only supplies the vendored sources and the stubbed tree.
  vendorDir = craneLib.vendorCargoDeps { cargoLock = ../Cargo.lock; };

  # Crate sources change with every commit, the dependencies they pull in do not.
  # Compiling them once against stub sources gives the dependency stage a key
  # that only moves when a manifest or the lockfile moves.
  dummySrc = craneLib.mkDummySrc { src = src; };

  # A store path in the binary counts as a reference and drags the whole build
  # closure into an artifact that is meant to be store-free. Rewrite every input
  # to a fixed name; the build directory is rewritten in the build phase, where
  # its path is known.
  remap = lib.concatStringsSep " " [
    "--remap-path-prefix=${toolchain}=/build/toolchain"
    "--remap-path-prefix=${vendorDir}=/build/vendor"
    "--remap-path-prefix=${pkgsOld.stdenv.cc}=/build/cc"
    "--remap-path-prefix=${pkgsOld.glibc}=/build/glibc"
    "--remap-path-prefix=${pkgsOld.fontconfig}=/build/fontconfig"
    "--remap-path-prefix=${pkgsOld.freetype}=/build/freetype"
    "--remap-path-prefix=${pkgsOld.libxkbcommon}=/build/xkbcommon"
    "--remap-path-prefix=${pkgsOld.xorg.libX11}=/build/x11"
    "--remap-path-prefix=${pkgsOld.xorg.libxcb}=/build/xcb"
  ];

  shared = {
    strictDeps = true;
    dontConfigure = true;
    # patchelf runs after the link; this stdenv must not add store paths back.
    dontFixup = true;

    # The old compiler decides which glibc symbols the link records.
    CC = "${pkgsOld.stdenv.cc}/bin/cc";
    CXX = "${pkgsOld.stdenv.cc}/bin/c++";
    CMAKE_C_COMPILER = "${pkgsOld.stdenv.cc}/bin/cc";
    CMAKE_CXX_COMPILER = "${pkgsOld.stdenv.cc}/bin/c++";
    CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_LINKER = "${pkgsOld.stdenv.cc}/bin/cc";

    nativeBuildInputs = [
      pkgsOld.cmake
      pkgsOld.patchelf
      pkgsOld.perl
      pkgsOld.pkg-config
      toolchain
    ];

    buildInputs = [
      pkgsOld.fontconfig
      pkgsOld.freetype
      pkgsOld.libxkbcommon
      pkgsOld.xorg.libX11
      pkgsOld.xorg.libxcb
    ];
  };

  environment = ''
    # The rust toolchain propagates its own gcc into PATH, so `cc` would
    # otherwise be the compiler of the current nixpkgs and record symbols from
    # its glibc. Put the 22.11 toolchain first for every build script.
    export PATH="${pkgsOld.stdenv.cc}/bin:${pkgsOld.binutils}/bin:$PATH"

    export RUSTFLAGS="--remap-path-prefix=$PWD=/build/src --remap-path-prefix=$NIX_BUILD_TOP=/build ${remap}"

    export CARGO_HOME="$TMPDIR/cargo-home"
    mkdir -p "$CARGO_HOME"
    cat ${vendorDir}/config.toml >> "$CARGO_HOME/config.toml"
  '';

  deps = pkgsOld.stdenv.mkDerivation (
    shared
    // {
      pname = "axlauncher-legacy-deps";
      inherit version;

      src = dummySrc;

      buildPhase = environment + common.buildPhase { patchImages = false; };

      installPhase = ''
        mkdir -p "$out"
        cp -a target "$out/target"
      '';
    }
  );
in
{
  inherit deps;

  binary = pkgsOld.stdenv.mkDerivation (
    shared
    // {
      pname = "axlauncher-legacy";
      inherit version src meta;

      buildPhase = ''
        cp -a ${deps}/target target
        # The copy comes from the store, so cargo cannot take its build lock
        # until the tree is writable.
        chmod -R u+w target
      ''
      + environment
      + common.buildPhase { patchImages = true; };

      installPhase = common.installPhase + ''
        patchelf --set-interpreter /lib64/ld-linux-x86-64.so.2 \
          --set-rpath '$ORIGIN/../lib' "$out/bin/axlauncher"
      '';
    }
  );
}

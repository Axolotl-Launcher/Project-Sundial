# SPDX-FileCopyrightText: 2026 Axolotl Team
# SPDX-License-Identifier: GPL-3.0-only

# Two builds of the same desktop binary.
#
# `newest` links against nixpkgs' glibc and runs under nix, so its wrapper is
# the user-facing package. `legacy` links against the glibc of the oldest
# supported distribution (Ubuntu 22.04, 2.35) and carries a system interpreter
# plus `$ORIGIN/../lib`, so a downstream packager can ship it as a native
# binary.
#
# Only cargo inputs are in `src`: a change to a flake file, a README or a
# workflow must not invalidate the dependency build.

{
  craneLib,
  lib,
  pkgs,
  pkgsOld,
  toolchain,
}:
let
  src = lib.fileset.toSource {
    root = ./.;
    fileset = lib.fileset.unions [
      ./Cargo.lock
      ./.cargo/config.toml
      # apps/ui embeds the window icon with include_bytes!.
      ./packaging/icons
      (lib.fileset.fileFilter (file: file.name == "Cargo.toml") ./.)
      (lib.fileset.fileFilter (file: file.hasExt "rs") ./.)
    ];
  };

  manifest = lib.importTOML "${src}/Cargo.toml";
  version = manifest.workspace.package.version;

  # Symbols baked at link time decide the floor, so this is the oldest glibc the
  # legacy build may be linked against.
  glibcFloor = "2.35";

  meta = {
    description = "Axolotl Launcher, a Minecraft launcher";
    homepage = manifest.workspace.package.repository;
    license = lib.licenses.gpl3Only;
    mainProgram = "axlauncher";
    platforms = [ "x86_64-linux" ];
  };

  commonArgs = {
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
    # them, so their store paths only feed the store rpath below.
    buildInputs = with pkgs; [
      fontconfig
      freetype
      libx11
      libxcb
      libxkbcommon
      vulkan-loader
      wayland
    ];
    cargoLock = ./Cargo.lock;
    env.CARGO_NET_OFFLINE = "true";
    doCheck = false;
    # installPhase handles the three artifacts itself; crane's build-log hook
    # would otherwise expect `cargo build` output on stdout.
    doNotPostBuildInstallCargoBinaries = true;
    # The final rpath is set after the link, where the fixup would otherwise
    # shrink it away.
    dontPatchELF = true;
  };

  cargoArtifacts = craneLib.buildDepsOnly commonArgs;

  # The platform binary embeds both worker libraries with include_bytes!, so
  # their bytes are final before it links.
  buildPhase = dist: ''
    runHook preBuild

    cargo rustc --locked --offline --release -p axlauncher_download_engine \
      --lib --crate-type cdylib --features worker
    cargo rustc --locked --offline --release -p axlauncher_accounts \
      --lib --crate-type cdylib

    ${lib.optionalString dist ''
      # A worker image runs under the target loader, so it may not point at the store.
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

  # NixOS keeps the host graphics driver outside the store.
  storeRpath = lib.concatStringsSep ":" [
    "${pkgs.wayland}/lib"
    "${pkgs.vulkan-loader}/lib"
    "/run/opengl-driver/lib"
  ];

  newest = craneLib.buildPackage (
    commonArgs
    // {
      inherit cargoArtifacts installPhase;
      buildPhase = buildPhase false;
      postFixup = ''
        patchelf --add-rpath '${storeRpath}' "$out/bin/axlauncher"
      '';
    }
  );

  # crane cannot be instantiated on the 22.11 package set, so the legacy build
  # is a plain derivation: the old stdenv links, the current toolchain
  # compiles, and crane only supplies the vendored sources.
  vendorDir = craneLib.vendorCargoDeps { cargoLock = ./Cargo.lock; };

  # A store path in the binary counts as a reference and drags the whole build
  # closure into an artifact that is meant to be store-free. Rewrite every input
  # to a fixed name; the build directory is rewritten in the build phase, where
  # its path is known.
  legacyRemap = lib.concatStringsSep " " [
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

  legacy = pkgsOld.stdenv.mkDerivation {
    pname = "axlauncher-legacy";
    inherit version src meta;
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

    buildPhase = ''
      runHook preBuild

      # The rust toolchain propagates its own gcc into PATH, so `cc` would
      # otherwise be the compiler of the current nixpkgs and record symbols from
      # its glibc. Put the 22.11 toolchain first for every build script.
      export PATH="${pkgsOld.stdenv.cc}/bin:${pkgsOld.binutils}/bin:$PATH"

      export RUSTFLAGS="--remap-path-prefix=$PWD=/build/src --remap-path-prefix=$NIX_BUILD_TOP=/build ${legacyRemap}"

      export CARGO_HOME="$TMPDIR/cargo-home"
      mkdir -p "$CARGO_HOME"
      cat ${vendorDir}/config.toml >> "$CARGO_HOME/config.toml"

      ${buildPhase true}

      runHook postBuild
    '';

    installPhase = installPhase + ''
      patchelf --set-interpreter /lib64/ld-linux-x86-64.so.2 \
        --set-rpath '$ORIGIN/../lib' "$out/bin/axlauncher"
    '';
  };

  # Self-contained tree for distributions that have no nix: the binary plus the
  # loader, the libraries it links and the keymap data libxkbcommon reads.
  # Fonts, drivers and D-Bus stay on the host.
  portable =
    pkgs.runCommand "axlauncher-portable-${version}"
      {
        nativeBuildInputs = [
          pkgs.binutils
          pkgs.patchelf
        ];
        inherit meta;
      }
      ''
        mkdir -p "$out/bin" "$out/libexec" "$out/lib" "$out/share"

        install -Dm755 ${newest}/bin/axlauncher "$out/libexec/axlauncher"
        install -Dm755 ${./packaging/linux/portable-launcher.sh} "$out/bin/axlauncher"

        libdirs="${pkgs.glibc}/lib:${pkgs.gcc-unwrapped.lib}/lib:${pkgs.libxkbcommon}/lib:${pkgs.libxcb}/lib"
        loader=$(readlink -f ${pkgs.glibc}/lib/ld-linux-x86-64.so.2)

        # Copy exactly what the binary links, resolved against the store.
        "$loader" --library-path "$libdirs" --list "$out/libexec/axlauncher" >store.txt
        if grep -q 'not found' store.txt; then
          cat store.txt >&2
          exit 1
        fi
        sed -n 's/^ *[^ ]* => \([^ ]*\) (0x[0-9a-f]*)$/\1/p' store.txt |
          while read -r file; do
            cp -L "$file" "$out/lib/$(basename "$file")"
          done
        cp -L "$loader" "$out/lib/ld-linux-x86-64.so.2"

        # Loaded by name at runtime, so the loader never lists them.
        for module in ${pkgs.glibc}/lib/libnss_*.so.*; do
          cp -L "$module" "$out/lib/"
        done
        cp -a ${pkgs.xkeyboard-config}/share/X11/xkb "$out/share/xkb"

        # The copied libraries keep the store paths they were linked against;
        # the bundled loader finds everything through --library-path.
        for library in "$out"/lib/*.so*; do
          chmod u+w "$library"
          patchelf --remove-rpath "$library"
        done

        patchelf --set-rpath '$ORIGIN/../lib' "$out/libexec/axlauncher"

        # The tree is self-contained when the bundled loader resolves every
        # dependency from inside it.
        "$out/lib/ld-linux-x86-64.so.2" --library-path "$out/lib" \
          --list "$out/libexec/axlauncher" >resolved.txt
        if grep ' => ' resolved.txt | grep -qv "$out/lib/"; then
          cat resolved.txt >&2
          exit 1
        fi

        install -Dm644 resolved.txt "$out/share/doc/linked-libraries.txt"
      '';

  wrapper = pkgs.runCommand "axlauncher-${version}" { inherit meta; } ''
    mkdir -p "$out/bin"
    ln -s ${newest}/bin/axlauncher "$out/bin/axlauncher"
    install -Dm644 ${./packaging/linux/com.axolotl.axlauncher.desktop} \
      "$out/share/applications/com.axolotl.axlauncher.desktop"
    install -Dm644 ${./packaging/icons/axolotl-launcher.png} \
      "$out/share/icons/hicolor/512x512/apps/axolotl-launcher.png"
  '';

  legacy-elf =
    pkgs.runCommand "axlauncher-legacy-elf-check"
      {
        nativeBuildInputs = [ pkgs.binutils ];
      }
      ''
        binary=${legacy}/bin/axlauncher

        interpreter=$(readelf -l "$binary" | sed -n 's/.*Requesting program interpreter: \(.*\)]/\1/p')
        test "$interpreter" = /lib64/ld-linux-x86-64.so.2

        if readelf -d "$binary" | grep -q /nix/store; then
          echo "legacy ELF still references the store" >&2
          exit 1
        fi

        floor=$(objdump -T "$binary" | grep -o 'GLIBC_[0-9.]*' | sort -uV | tail -1)
        echo "interpreter=$interpreter glibc_floor=$floor" > "$out"

        if [ "$(printf '%s\nGLIBC_${glibcFloor}\n' "$floor" | sort -V | tail -1)" != "GLIBC_${glibcFloor}" ]; then
          echo "legacy ELF needs $floor, above GLIBC_${glibcFloor}" >&2
          exit 1
        fi
      '';
in
{
  bin = {
    inherit legacy newest;
  };

  packages = {
    axlauncher = wrapper;
    axlauncher-legacy = legacy;
    axlauncher-cargo-deps = cargoArtifacts;
    axlauncher-newest = newest;
    axlauncher-portable = portable;
  };

  checks = {
    axlauncher = wrapper;
    legacy-elf = legacy-elf;
    portable = portable;
  };
}

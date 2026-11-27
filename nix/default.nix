# SPDX-FileCopyrightText: 2026 Axolotl Team
# SPDX-License-Identifier: GPL-3.0-only

# Assembles the artifact set. One file per artifact, because each answers a
# different question: which glibc, which layout, which packaging step.

{
  craneLib,
  lib,
  pkgs,
  pkgsOld,
  toolchain,
}:
let
  common = import ./common.nix { inherit lib; };
  newest = import ./newest.nix {
    inherit
      craneLib
      common
      lib
      pkgs
      ;
  };
  legacy = import ./legacy.nix {
    inherit
      common
      craneLib
      lib
      pkgs
      pkgsOld
      toolchain
      ;
  };
  portable = import ./portable.nix {
    inherit pkgs;
    newest = newest.binary;
    inherit (common) version meta;
  };
  wrapper = import ./wrapper.nix {
    inherit pkgs;
    newest = newest.binary;
    inherit (common) version meta;
  };
  checks = import ./checks.nix {
    inherit legacy pkgs;
    inherit (common) glibcFloor;
  };
in
{
  bin = {
    inherit legacy;
    newest = newest.binary;
  };

  packages = {
    axlauncher = wrapper;
    axlauncher-cargo-deps = newest.deps;
    axlauncher-legacy = legacy;
    axlauncher-newest = newest.binary;
    axlauncher-portable = portable;
  };

  checks = checks // {
    axlauncher = wrapper;
    inherit portable;
  };
}

# SPDX-FileCopyrightText: 2026 Axolotl Team
# SPDX-License-Identifier: GPL-3.0-only

{
  description = "Axolotl Launcher";

  inputs = {
    crane.url = "github:ipetkov/crane";
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    nixpkgs-old.url = "github:NixOS/nixpkgs/nixos-22.11";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  # The public binary cache. `nixConfig` is applied only with
  # `--accept-flake-config`; CONTRIBUTING.md documents the explicit flags.
  nixConfig = {
    extra-substituters = [ "https://axlmc.cachix.org" ];
    extra-trusted-public-keys = [
      "axlmc.cachix.org-1:dmsRJqFQqLMfvRveerHV3DxDCW1wq1N+TX1+dryYWZU="
    ];
  };

  outputs =
    {
      self,
      crane,
      nixpkgs,
      nixpkgs-old,
      rust-overlay,
    }:
    let
      inherit (nixpkgs) lib;
      systems = [ "x86_64-linux" ];

      perSystem =
        system:
        let
          pkgs = import nixpkgs {
            inherit system;
            overlays = [ rust-overlay.overlays.default ];
          };
          # The legacy build links against the oldest supported glibc.
          pkgsOld = import nixpkgs-old { inherit system; };
          toolchain = pkgs.rust-bin.stable."1.100.0".default.override {
            extensions = [ "rust-src" ];
          };
          package = import ./package.nix {
            inherit pkgs pkgsOld toolchain;
            inherit (pkgs) lib;
            craneLib = (crane.mkLib pkgs).overrideToolchain toolchain;
          };
        in
        {
          packages = package.packages // {
            default = package.packages.axlauncher;
          };

          inherit (package) checks;

          bin = package.bin;

          apps.default = {
            type = "app";
            program = "${package.packages.axlauncher}/bin/axlauncher";
            meta.description = "Axolotl Launcher";
          };

          devShells.default = pkgs.mkShell {
            inputsFrom = [ package.bin.newest ];
            packages = [
              toolchain
              pkgs.nodejs_24
            ];
          };

          formatter = pkgs.nixfmt;
        };
    in
    {
      packages = lib.genAttrs systems (system: (perSystem system).packages);
      checks = lib.genAttrs systems (system: (perSystem system).checks);
      # Raw binaries; `newest` is what the wrapper and the AppImage use, `legacy`
      # only feeds native packaging and is not published to the binary cache.
      __axlbin = lib.genAttrs systems (system: (perSystem system).bin);
      apps = lib.genAttrs systems (system: (perSystem system).apps);
      devShells = lib.genAttrs systems (system: (perSystem system).devShells);
      formatter = lib.genAttrs systems (system: (perSystem system).formatter);
    };
}

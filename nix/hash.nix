# SPDX-FileCopyrightText: 2026 Axolotl Team
# SPDX-License-Identifier: GPL-3.0-only

# Fixed-output hashes, kept apart so that bumping one touches a single file and
# a hash bump never shows up mixed into a derivation change.
#
# Nothing needs one yet: the flake pins its inputs in `flake.lock`, and the
# crates come from their checksums in `Cargo.lock`. Add an entry here and pass
# `import ./hash.nix` down when a derivation starts fetching something by hand,
# for example:
#
#   {
#     "vendor" = "sha256-AAAA…=";
#   }
{
}

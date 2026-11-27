# SPDX-FileCopyrightText: 2026 Axolotl Team
# SPDX-License-Identifier: GPL-3.0-only

# Gates the release artifact has to pass: the ELF must load on a machine without
# nix, and it must not require a newer glibc than the oldest supported release.

{
  glibcFloor,
  legacy,
  pkgs,
}:
{
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
}

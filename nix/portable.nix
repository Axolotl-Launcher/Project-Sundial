# SPDX-FileCopyrightText: 2026 Axolotl Team
# SPDX-License-Identifier: GPL-3.0-only

# A self-contained tree for distributions without nix: the binary plus the
# loader, the libraries it links and the keymap data libxkbcommon reads. Fonts,
# drivers and D-Bus stay on the host.

{
  newest,
  pkgs,
  version,
  meta,
}:
pkgs.runCommand "axlauncher-portable-${version}"
  {
    nativeBuildInputs = [
      pkgs.binutils
      pkgs.gcc
      pkgs.patchelf
    ];
    inherit meta;
  }
  ''
      mkdir -p "$out/bin" "$out/libexec" "$out/lib" "$out/share"

      install -Dm755 ${newest}/bin/axlauncher "$out/libexec/axlauncher"
      install -Dm755 ${../packaging/linux/portable-launcher.sh} "$out/bin/axlauncher"

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
      # The package publishes the data as a symlink into its own tree, so a
      # bundle copies the trees it points at instead.
      cp -rL ${pkgs.xkeyboard-config}/share/X11/xkb "$out/share/xkb"

      # The copied libraries keep the store paths they were linked against; the
      # bundled loader finds everything through --library-path.
      for library in "$out"/lib/*.so*; do
        chmod u+w "$library"
        patchelf --remove-rpath "$library"
      done

      patchelf --set-rpath '$ORIGIN/../lib' "$out/libexec/axlauncher"

      # The tree is self-contained when the bundled loader resolves every dependency
      # from inside it.
      "$out/lib/ld-linux-x86-64.so.2" --library-path "$out/lib" \
        --list "$out/libexec/axlauncher" >resolved.txt
      if grep ' => ' resolved.txt | grep -qv "$out/lib/"; then
        cat resolved.txt >&2
        exit 1
      fi

      # The launcher must survive an exec that does not fall back to a shell; the
      # probe exits 2 when the kernel rejects the file.
      cat >probe.c <<'PROBE'
      #include <unistd.h>
      int main(int argc, char **argv) { execv(argv[1], &argv[1]); return 2; }
      PROBE
      cc -o probe probe.c
      status=0
      ./probe "$out/bin/axlauncher" --axlauncher-worker bogus /tmp/x || status=$?
      test "$status" = 1

      # Load the exact embedded image through the portable entry, then shut down.
      # The probe image is a build input only, not part of the portable tree.
      scratch=$(mktemp -d)
      cp ${newest.workerProbe}/java-discovery-image "$scratch/libjava-discovery.so"
      printf '\012\000\000\000"Shutdown"' |
        ./probe "$out/bin/axlauncher" --axlauncher-worker java-discovery \
          "$scratch/libjava-discovery.so" > "$scratch/reply"
      printf '\007\000\000\000"Ready"' | cmp - "$scratch/reply"

      install -Dm644 resolved.txt "$out/share/doc/linked-libraries.txt"
      # Every step above runs only if the heredoc closed; the doc file proves it did.
      test -f "$out/share/doc/linked-libraries.txt"
  ''

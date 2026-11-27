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
    cp -a ${pkgs.xkeyboard-config}/share/X11/xkb "$out/share/xkb"

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

    install -Dm644 resolved.txt "$out/share/doc/linked-libraries.txt"
  ''

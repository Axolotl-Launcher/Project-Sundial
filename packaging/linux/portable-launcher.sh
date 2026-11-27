#!/bin/sh
# SPDX-FileCopyrightText: 2026 Axolotl Team
# SPDX-License-Identifier: GPL-3.0-only
#
# Runs the bundled binary through the bundled loader: the interpreter baked into
# the ELF points at the nix store, which does not exist on the target machine.
# The host directories stay in the search path so the driver libraries the
# binary dlopens (Vulkan, wayland, EGL) resolve from the host.

set -eu

here=$(dirname "$(readlink -f "$0")")/..
host_paths="/usr/lib/x86_64-linux-gnu:/usr/lib64:/usr/lib:/lib64:/lib"

# A worker starts by re-executing the launcher, and under a loader
# `/proc/self/exe` names the loader, so the child is pointed at this script.
AXLAUNCHER_EXECUTABLE="$here/bin/axlauncher"
export AXLAUNCHER_EXECUTABLE

if [ -d "$here/share/xkb" ]; then
  XKB_CONFIG_ROOT="$here/share/xkb"
  export XKB_CONFIG_ROOT
fi

exec "$here/lib/ld-linux-x86-64.so.2" \
  --library-path "$here/lib:$host_paths" \
  "$here/libexec/axlauncher" "$@"

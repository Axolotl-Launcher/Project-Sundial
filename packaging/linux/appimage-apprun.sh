#!/bin/sh
# SPDX-FileCopyrightText: 2026 Axolotl Team
# SPDX-License-Identifier: GPL-3.0-only
#
# AppRun for the AppImage: the bundle root holds the portable tree, and its
# `bin/axlauncher` already knows how to start the binary through the bundled
# loader.

set -eu

here=$(dirname "$(readlink -f "$0")")

exec "$here/bin/axlauncher" "$@"

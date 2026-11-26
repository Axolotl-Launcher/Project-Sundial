// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

fn main() {
    println!("cargo:rerun-if-changed=axlauncher.rc");
    println!("cargo:rerun-if-changed=../../packaging/icons/axolotl-launcher.ico");
    embed_resource::compile_for("axlauncher.rc", ["axlauncher"], embed_resource::NONE)
        .manifest_required()
        .expect("embed the Windows application icon");
}

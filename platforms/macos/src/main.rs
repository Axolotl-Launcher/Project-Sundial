//! macOS entry point. All UI construction lives in `superlogical_frontend`.
//!
//! The macOS bundle layout (Info.plist, icon, code signing) is produced around
//! this binary; see `packaging/macos`.

fn main() {
    superlogical_frontend::run();
}

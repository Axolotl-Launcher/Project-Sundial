//! Windows entry point. All UI construction lives in `superlogical_frontend`.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

fn main() {
    superlogical_frontend::run();
}

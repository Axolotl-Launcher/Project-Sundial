//! Windows entry point. All UI construction lives in `superlogical_ui`.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

fn main() {
    superlogical_ui::run();
}

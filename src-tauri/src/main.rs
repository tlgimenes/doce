// Prevents an additional console window on Windows in release builds. Not
// relevant to doce's macOS-only target, kept for Tauri convention parity.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    doce_lib::run();
}

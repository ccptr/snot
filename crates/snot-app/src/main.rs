// Keep the console off on Windows release builds; a notes app has no CLI face.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    snot_lib::run()
}

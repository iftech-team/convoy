// A console window would appear behind the app on Windows without this.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    convoy_tauri::run()
}

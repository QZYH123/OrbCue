#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

#[cfg(windows)]
fn main() {
    orbcue_lib::run();
}

#[cfg(not(windows))]
fn main() {
    eprintln!("OrbCue desktop is Windows-only");
    std::process::exit(1);
}

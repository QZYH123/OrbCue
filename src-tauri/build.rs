fn main() {
    ensure_wsl_dock_resource();
    tauri_build::build()
}

fn ensure_wsl_dock_resource() {
    let path = std::path::Path::new("resources/dock-wsl");
    if path.is_file() {
        return;
    }
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Err(error) = std::fs::write(path, b"") {
        println!("cargo:warning=could not create dock-wsl placeholder: {error}");
    }
}

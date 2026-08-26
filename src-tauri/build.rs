fn main() {
    ensure_wsl_orb_resource();
    tauri_build::build()
}

fn ensure_wsl_orb_resource() {
    let path = std::path::Path::new("resources/orb-wsl");
    if path.is_file() {
        return;
    }
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Err(error) = std::fs::write(path, b"") {
        println!("cargo:warning=could not create orb-wsl placeholder: {error}");
    }
}

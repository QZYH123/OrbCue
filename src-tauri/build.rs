fn main() {
    ensure_wsl_orb_resource();
    ensure_sidecar_placeholder();
    tauri_build::build()
}

fn ensure_sidecar_placeholder() {
    let Ok(target) = std::env::var("TARGET") else {
        return;
    };
    if target.is_empty() {
        return;
    }
    let exe = if target.contains("windows") {
        ".exe"
    } else {
        ""
    };
    let path = std::path::PathBuf::from("binaries").join(format!("orb-{target}{exe}"));
    if path.is_file() {
        return;
    }
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Err(error) = std::fs::write(&path, b"") {
        println!("cargo:warning=could not create sidecar placeholder: {error}");
    }
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

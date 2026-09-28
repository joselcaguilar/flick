fn main() {
    ensure_sidecar_placeholder();
    tauri_build::build();
}

fn ensure_sidecar_placeholder() {
    let Ok(target) = std::env::var("TARGET") else {
        return;
    };
    let Ok(manifest_dir) = std::env::var("CARGO_MANIFEST_DIR") else {
        return;
    };
    let exe = if target.contains("windows") {
        ".exe"
    } else {
        ""
    };
    let path = std::path::Path::new(&manifest_dir)
        .join("binaries")
        .join(format!("flick-engine-{target}{exe}"));
    if path.exists() {
        return;
    }
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(
        &path,
        b"placeholder; apps/desktop/scripts/prepare-sidecar.mjs overwrites this file\n",
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Ok(metadata) = std::fs::metadata(&path) {
            let mut permissions = metadata.permissions();
            permissions.set_mode(0o755);
            let _ = std::fs::set_permissions(&path, permissions);
        }
    }
}

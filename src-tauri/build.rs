fn main() {
    tauri_build::build();
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
        let out_dir = std::env::var("OUT_DIR").unwrap();
        let rc_path = std::path::Path::new(&manifest_dir).join("manifest.rc");
        let out_res = std::path::Path::new(&out_dir).join("manifest.res");

        let compiled = if rc_path.exists() {
            let windres_ok = std::process::Command::new("windres")
                .current_dir(&manifest_dir)
                .args(["-i", "manifest.rc", "-o"])
                .arg(&out_res)
                .status()
                .map(|s| s.success())
                .unwrap_or(false);

            if windres_ok {
                true
            } else {
                std::process::Command::new("rc")
                    .current_dir(&manifest_dir)
                    .args(["/fo"])
                    .arg(&out_res)
                    .arg("manifest.rc")
                    .status()
                    .map(|s| s.success())
                    .unwrap_or(false)
            }
        } else {
            false
        };

        let target_res = if compiled && out_res.exists() {
            Some(out_res)
        } else {
            let fallback = std::path::Path::new(&manifest_dir).join("manifest.res");
            if fallback.exists() {
                Some(fallback)
            } else {
                None
            }
        };

        if let Some(res) = target_res {
            println!("cargo:rustc-link-arg-tests={}", res.display());
        }
    }
}

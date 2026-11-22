use std::process::Command;

fn main() {
    println!("cargo:rustc-link-lib=SDL2");
    if let Ok(lib_dir) = std::env::var("QUOS_SDL2_LIB_DIR") {
        println!("cargo:rustc-link-search=native={}", lib_dir);
    }

    println!("cargo:rerun-if-env-changed=YASUMU_BUILD_ID");
    println!("cargo:rerun-if-changed=.git/HEAD");

    let pkg_ver = env!("CARGO_PKG_VERSION");
    let build_id = std::env::var("YASUMU_BUILD_ID")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .or_else(|| {
            Command::new("git")
                .args(["rev-parse", "--short", "HEAD"])
                .output()
                .ok()
                .filter(|o| o.status.success())
                .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
                .filter(|s| !s.is_empty())
        })
        .or_else(|| {
            Command::new("date")
                .args(["-u", "+%Y%m%d"])
                .output()
                .ok()
                .filter(|o| o.status.success())
                .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
                .filter(|s| !s.is_empty())
        })
        .unwrap_or_else(|| "dev".to_string());

    println!("cargo:rustc-env=YASUMU_VERSION=v{} ({})", pkg_ver, build_id);
}

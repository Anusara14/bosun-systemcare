//! Embeds the icon/manifest, and the release `bosun.exe` + `bosun-tray.exe`
//! from `BOSUN_PAYLOAD_DIR` (set by `scripts/release.ps1`). Without it the
//! payload is empty and the setup refuses to run, so plain workspace builds
//! still compile.

use std::path::PathBuf;

fn main() {
    println!("cargo:rerun-if-changed=res/setup.rc");
    println!("cargo:rerun-if-changed=res/setup.manifest");
    println!("cargo:rerun-if-env-changed=BOSUN_PAYLOAD_DIR");
    embed_resource::compile("res/setup.rc", embed_resource::NONE)
        .manifest_required()
        .expect("compile Windows resources");
    let out = PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR"));
    let payload = std::env::var("BOSUN_PAYLOAD_DIR").ok().map(PathBuf::from);
    for name in ["bosun.exe", "bosun-tray.exe"] {
        let bytes = match &payload {
            Some(dir) => {
                let src = dir.join(name);
                println!("cargo:rerun-if-changed={}", src.display());
                std::fs::read(&src).unwrap_or_else(|e| panic!("payload {}: {e}", src.display()))
            }
            None => Vec::new(),
        };
        std::fs::write(out.join(name), bytes).expect("write payload");
    }
}

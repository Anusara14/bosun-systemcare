fn main() {
    println!("cargo:rerun-if-changed=res/tray.rc");
    println!("cargo:rerun-if-changed=res/app.manifest");
    embed_resource::compile("res/tray.rc", embed_resource::NONE)
        .manifest_required()
        .expect("compile Windows resources");
}

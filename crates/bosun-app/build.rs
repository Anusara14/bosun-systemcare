fn main() {
    println!("cargo:rerun-if-changed=res/app.rc");
    println!("cargo:rerun-if-changed=res/app.manifest");
    // Re-embed when the brand icons are re-exported.
    println!("cargo:rerun-if-changed=../../brand/export/ico");
    embed_resource::compile("res/app.rc", embed_resource::NONE)
        .manifest_required()
        .expect("compile Windows resources");
}

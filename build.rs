fn main() {
    println!("cargo:rerun-if-changed=resources/macos/Info.plist");
    println!("cargo:rerun-if-changed=assets/icon/velotype.ico");
    println!("cargo:rerun-if-changed=assets/icon/toolbar");

    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        embed_resource::compile("resources/windows/velotype.rc", embed_resource::NONE)
            .manifest_optional()
            .expect("failed to compile Velotype Windows resources");
    }
}

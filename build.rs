fn main() {
    println!("cargo:rerun-if-changed=resources/macos/Info.plist");
    println!("cargo:rerun-if-changed=assets/icon/appicon.png");
    println!("cargo:rerun-if-changed=assets/icon/velotype.ico");
    println!("cargo:rerun-if-changed=resources/macos/markman.icns");
    println!("cargo:rerun-if-changed=assets/icon/toolbar");
    println!("cargo:rerun-if-changed=assets/fonts/SourceHanSansSC-Regular.otf");

    let font_path = std::path::Path::new("assets/fonts/SourceHanSansSC-Regular.otf");
    if !font_path.exists() {
        println!(
            "cargo:warning=Missing assets/fonts/SourceHanSansSC-Regular.otf — run ./scripts/fetch_mermaid_font.sh"
        );
    }

    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        embed_resource::compile("resources/windows/markman.rc", embed_resource::NONE)
            .manifest_optional()
            .expect("failed to compile Markman Windows resources");
    }
}

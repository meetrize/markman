//! Application icon installation for platforms that do not load bundle resources
//! from a bare executable (notably macOS Dock / app switcher when running via
//! `cargo run` without a `.app` wrapper).

#[cfg(target_os = "macos")]
const APP_ICON_PNG: &[u8] = include_bytes!("../assets/icon/appicon.png");

/// Install the embedded Markman icon as the process application icon.
#[cfg(target_os = "macos")]
pub(crate) fn install() {
    use objc2::AnyThread;
    use objc2::MainThreadMarker;
    use objc2_app_kit::{NSApplication, NSImage};
    use objc2_foundation::NSData;

    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };

    let data = unsafe {
        NSData::dataWithBytes_length(
            APP_ICON_PNG.as_ptr().cast(),
            APP_ICON_PNG.len(),
        )
    };
    let Some(image) = NSImage::initWithData(NSImage::alloc(), &data) else {
        eprintln!("failed to decode embedded application icon");
        return;
    };

    let app = NSApplication::sharedApplication(mtm);
    unsafe {
        app.setApplicationIconImage(Some(&image));
    }
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn install() {}

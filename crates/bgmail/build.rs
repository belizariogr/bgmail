//! Build script: embeds the app icon in the Windows executable (Explorer,
//! taskbar, Alt+Tab, installer shortcuts). A no-op on every other platform —
//! macOS/Linux packages carry the icon in the bundle / hicolor theme instead.

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    // `cfg(windows)` is the host (where `embed-resource` is available); the
    // target check keeps a Windows host building for another OS untouched.
    #[cfg(windows)]
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        embed_windows_icon();
    }
}

#[cfg(windows)]
fn embed_windows_icon() {
    use std::path::PathBuf;

    let manifest_dir =
        PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    let icon = manifest_dir.join("../../assets/appIcon/BGMail.ico");
    println!("cargo:rerun-if-changed={}", icon.display());

    // Generated so the icon path is absolute (rc.exe resolves relative paths
    // against its own working directory); backslashes are escaped for RC.
    let out_dir = PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR"));
    let rc = out_dir.join("bgmail-icon.rc");
    let icon_path = icon.display().to_string().replace('\\', "\\\\");
    std::fs::write(&rc, format!("1 ICON \"{icon_path}\"\n")).expect("write bgmail-icon.rc");

    // GPUI already embeds the application manifest; this resource only adds
    // the icon, so a manifest is not required here.
    embed_resource::compile(&rc, embed_resource::NONE)
        .manifest_optional()
        .expect("compile Windows icon resource");
}

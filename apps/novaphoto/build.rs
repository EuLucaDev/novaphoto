//! Windows only: embed the app icon and version info (VERSIONINFO) into `novaphoto.exe`.
//!
//! On every other target this does nothing. A missing resource compiler is a warning, so a
//! cross-compile from macOS or Linux still links, unless `NOVAPHOTO_REQUIRE_WINRES=1` (set by the
//! release workflow) turns it into an error.

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=../../assets/app-icon/novaphoto.ico");
    println!("cargo:rerun-if-env-changed=NOVAPHOTO_REQUIRE_WINRES");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let mut res = winresource::WindowsResource::new();
    res.set_icon("../../assets/app-icon/novaphoto.ico")
        .set("ProductName", "Nova Photo")
        .set("FileDescription", "Nova Photo image editor")
        .set("CompanyName", "Nova Foods")
        .set("LegalCopyright", "Copyright (c) the Nova Photo authors. MIT OR Apache-2.0.")
        .set("OriginalFilename", "novaphoto.exe")
        .set("InternalName", "novaphoto");
    if let Err(e) = res.compile() {
        if std::env::var_os("NOVAPHOTO_REQUIRE_WINRES").is_some() {
            panic!("embedding Windows resources failed: {e}");
        }
        println!("cargo:warning=novaphoto.exe built without icon/version resources: {e}");
    }
}

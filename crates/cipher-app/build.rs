//! On Windows, puts the icon and version info into `cipher.exe` (Explorer, the taskbar and
//! Alt+Tab read them from there). Other platforms take the icon from their package.

fn main() {
    let ico = "../../packaging/icons/out/cipher.ico";
    println!("cargo:rerun-if-changed={ico}");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let mut res = winresource::WindowsResource::new();
    res.set_icon(ico)
        .set("ProductName", "Cipher")
        .set("FileDescription", "Cipher")
        .set("CompanyName", "Cipher")
        .set("LegalCopyright", "Cipher contributors");
    res.compile()
        .expect("compile the Windows resources (needs rc.exe or windres)");
}

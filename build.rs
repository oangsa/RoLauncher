fn main() {
    println!("cargo:rerun-if-changed=assets/RoLauncher.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        winresource::WindowsResource::new()
            .set_icon("assets/RoLauncher.ico")
            .set("ProductName", "RoLauncher")
            .set("FileDescription", "RoLauncher account launcher")
            .set("OriginalFilename", "RoLauncher.exe")
            .set("CompanyName", "oangsa")
            .compile()
            .expect("Unable to embed the Windows app icon/version resources");
    }
}

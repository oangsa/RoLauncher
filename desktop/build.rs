fn main() {
    println!("cargo:rerun-if-changed=../assets/RoLauncher.ico");
    println!("cargo:rerun-if-changed=../Cargo.toml");
    println!("cargo:rerun-if-changed=app.manifest");
    println!("cargo:rerun-if-changed=../scripts/apply-update.ps1");
    println!("cargo:rerun-if-changed=../CHANGELOG.md");

    #[cfg(target_os = "windows")]
    compile_resources();
}

#[cfg(target_os = "windows")]
fn compile_resources() {
    let version = read_root_version().unwrap_or_else(|| "0.0.0".to_string());
    println!("cargo:rustc-env=ROLAUNCHER_SHELL_VERSION={version}");
    let core = version.split('-').next().unwrap_or("0.0.0");
    let parts: Vec<u64> = core
        .split('.')
        .filter_map(|part| part.parse::<u64>().ok())
        .collect();
    let major = parts.first().copied().unwrap_or(0) & 0xFFFF;
    let minor = parts.get(1).copied().unwrap_or(0) & 0xFFFF;
    let patch = parts.get(2).copied().unwrap_or(0) & 0xFFFF;
    let packed = (major << 48) | (minor << 32) | (patch << 16);

    let mut res = winresource::WindowsResource::new();
    res.set_icon("../assets/RoLauncher.ico")
        .set_manifest_file("app.manifest")
        .set("ProductName", "RoLauncher")
        .set("FileDescription", "RoLauncher GPUI Desktop")
        .set("OriginalFilename", "RoLauncher.Desktop.exe")
        .set("CompanyName", "oangsa")
        .set("ProductVersion", &version)
        .set("FileVersion", core)
        .set_version_info(winresource::VersionInfo::FILEVERSION, packed)
        .set_version_info(winresource::VersionInfo::PRODUCTVERSION, packed);
    res.compile()
        .expect("Unable to embed Windows resources for RoLauncher.Desktop");
}

#[cfg(target_os = "windows")]
fn read_root_version() -> Option<String> {
    let cargo = std::fs::read_to_string("../Cargo.toml").ok()?;
    for line in cargo.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("version = \"")
            && let Some(ver) = rest.strip_suffix('"')
        {
            return Some(ver.to_string());
        }
    }
    None
}

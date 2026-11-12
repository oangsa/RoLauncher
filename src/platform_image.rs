// Native image checks never trust a process name or a path substring.
fn image_path(handle: HANDLE) -> Option<PathBuf> {
    let mut path = vec![0u16; 32768];
    let mut length = path.len() as u32;
    (unsafe { QueryFullProcessImageNameW(handle, 0, path.as_mut_ptr(), &mut length) } != 0)
        .then(|| PathBuf::from(String::from_utf16_lossy(&path[..length as usize])))
}
fn image_under_root(path: &std::path::Path, root: &std::path::Path) -> bool {
    let (Ok(path), Ok(root)) = (path.canonicalize(), root.canonicalize()) else {
        return false;
    };
    let path = path.to_string_lossy().to_lowercase();
    let root = root.to_string_lossy().to_lowercase();
    path.starts_with(&(root + "\\")) && path.ends_with("\\robloxplayerbeta.exe")
}
fn bootstrapper_under_root(path: &std::path::Path, root: &std::path::Path) -> bool {
    let (Ok(path), Ok(root)) = (path.canonicalize(), root.canonicalize()) else {
        return false;
    };
    let path = path.to_string_lossy().to_lowercase();
    let root = root.to_string_lossy().to_lowercase();
    path.starts_with(&(root + "\\"))
        && (path.ends_with("\\robloxplayerlauncher.exe")
            || path.ends_with("\\robloxplayerinstaller.exe"))
}
fn same_session(pid: u32) -> bool {
    let (mut current, mut actual) = (0, 0);
    unsafe {
        ProcessIdToSessionId(GetCurrentProcessId(), &mut current) != 0
            && ProcessIdToSessionId(pid, &mut actual) != 0
            && current == actual
    }
}
fn valid_player_image(handle: HANDLE) -> bool {
    let Some(path) = image_path(handle) else {
        return false;
    };
    let Some(local) = std::env::var_os("LOCALAPPDATA") else {
        return false;
    };
    image_under_root(&path, &PathBuf::from(local).join("Roblox").join("Versions"))
}
fn valid_bootstrapper_image(handle: HANDLE) -> bool {
    let Some(path) = image_path(handle) else {
        return false;
    };
    let Some(local) = std::env::var_os("LOCALAPPDATA") else {
        return false;
    };
    bootstrapper_under_root(&path, &PathBuf::from(local).join("Roblox").join("Versions"))
}

#[cfg(test)]
fn fixture_image(handle: HANDLE) -> bool {
    // Only test entry points pass this verifier. Production always uses the
    // fixed installation root, never a fixture path or an environment override.
    let Some(path) = image_path(handle) else {
        return false;
    };
    image_under_root(&path, &std::env::temp_dir())
        && path
            .to_string_lossy()
            .to_lowercase()
            .contains("\\roblox\\versions\\")
}

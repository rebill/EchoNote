use std::path::Path;

/// Bytes free for this user on the volume `path` is (or will be) on.
pub fn free_bytes(path: &Path) -> Option<u64> {
    let existing = path.ancestors().find(|dir| dir.exists())?;
    available(existing)
}

#[cfg(unix)]
fn available(path: &Path) -> Option<u64> {
    use std::os::unix::ffi::OsStrExt;
    let path = std::ffi::CString::new(path.as_os_str().as_bytes()).ok()?;
    let mut stat: libc::statvfs = unsafe { std::mem::zeroed() };
    // SAFETY: `path` is NUL-terminated and `stat` is a valid out pointer.
    if unsafe { libc::statvfs(path.as_ptr(), &mut stat) } != 0 {
        return None;
    }
    #[allow(clippy::unnecessary_cast)] // the field widths differ between platforms
    Some(stat.f_bavail as u64 * stat.f_frsize as u64)
}

#[cfg(windows)]
fn available(path: &Path) -> Option<u64> {
    use std::os::windows::ffi::OsStrExt;
    let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    let mut free = 0u64;
    // SAFETY: `wide` is NUL-terminated; the out pointers are valid or null.
    let ok = unsafe {
        windows_sys::Win32::Storage::FileSystem::GetDiskFreeSpaceExW(
            wide.as_ptr(),
            &mut free,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    };
    (ok != 0).then_some(free)
}

#[cfg(not(any(unix, windows)))]
fn available(_path: &Path) -> Option<u64> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_missing_path_reports_its_volume() {
        let dir = std::env::temp_dir();
        let free = free_bytes(&dir.join("echonote-not-there/deeper")).unwrap();
        assert!(free > 0);
        assert_eq!(free_bytes(&dir).map(|f| f > 0), Some(true));
    }
}

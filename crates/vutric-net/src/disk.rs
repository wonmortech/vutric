//! Free space on the disk a file would go to, for the receiver's free-space guard (ADR-0007 part 4).

use std::path::Path;

/// Bytes free for this user on the volume holding `path` (or its nearest existing parent); `None` when the
/// platform cannot tell, in which case the guard does not block.
#[must_use]
pub fn free_bytes(path: &Path) -> Option<u64> {
    let mut p = path;
    loop {
        if p.exists() {
            return free_at(p);
        }
        p = p.parent()?;
    }
}

#[cfg(windows)]
fn free_at(path: &Path) -> Option<u64> {
    use std::os::windows::ffi::OsStrExt as _;

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetDiskFreeSpaceExW(
            directory: *const u16,
            free_to_caller: *mut u64,
            total: *mut u64,
            total_free: *mut u64,
        ) -> i32;
    }

    let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    let mut free = 0u64;
    // SAFETY: `wide` is NUL-terminated and outlives the call; the out pointers are valid or null as the API
    // allows.
    let ok = unsafe {
        GetDiskFreeSpaceExW(
            wide.as_ptr(),
            &raw mut free,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    };
    (ok != 0).then_some(free)
}

#[cfg(unix)]
fn free_at(path: &Path) -> Option<u64> {
    use std::os::unix::ffi::OsStrExt as _;

    let c = std::ffi::CString::new(path.as_os_str().as_bytes()).ok()?;
    // SAFETY: an all-zero `statvfs` is a valid value to be overwritten.
    let mut st: libc::statvfs = unsafe { std::mem::zeroed() };
    // SAFETY: `c` is a NUL-terminated path that outlives the call; `st` is a valid out pointer.
    let rc = unsafe { libc::statvfs(c.as_ptr(), &raw mut st) };
    #[allow(clippy::useless_conversion)] // the field types differ between platforms
    (rc == 0).then(|| u64::from(st.f_bavail).saturating_mul(u64::from(st.f_frsize)))
}

#[cfg(not(any(windows, unix)))]
fn free_at(_: &Path) -> Option<u64> {
    None
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_temp_dir_has_some_room() {
        let free = super::free_bytes(&std::env::temp_dir().join("no").join("such").join("file"));
        assert!(free.is_some_and(|f| f > 0));
    }
}

//! Free space on the disk a file would go to, for the receiver's free-space guard (ADR-0007 part 4), and the
//! clean-up of unfinished part files (backlog T6).

use std::path::Path;
use std::time::{Duration, SystemTime};

/// What [`sweep_parts`] deleted.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Swept {
    pub files: u32,
    pub bytes: u64,
}

/// How deep [`sweep_parts`] looks below the save folder (split by type, per device, per day, and a kept tree).
pub const SWEEP_DEPTH: usize = 8;

/// Deletes unfinished `.vutric-part` files under `root` last written more than `keep_days` days before `now`
/// (`0` = all of them: "delete now"). Only plain files with that ending are touched; links are never followed,
/// and nothing deeper than [`SWEEP_DEPTH`] is visited. A part being written right now is newer than any
/// `keep_days >= 1`, so the daily sweep never races a transfer; "delete now" is for when nothing is receiving.
#[must_use]
pub fn sweep_parts(root: &Path, keep_days: u32, now: SystemTime) -> Swept {
    let cutoff = now
        .checked_sub(Duration::from_secs(u64::from(keep_days) * 86_400))
        .unwrap_or(SystemTime::UNIX_EPOCH);
    let mut out = Swept::default();
    let mut stack = vec![(root.to_path_buf(), 0usize)];
    while let Some((dir, depth)) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for e in entries.flatten() {
            let Ok(t) = e.file_type() else { continue };
            if t.is_dir() {
                if depth < SWEEP_DEPTH {
                    stack.push((e.path(), depth + 1));
                }
            } else if t.is_file()
                && e.file_name().to_string_lossy().ends_with(".vutric-part")
                && let Ok(m) = e.metadata()
                && (keep_days == 0 || m.modified().is_ok_and(|w| w < cutoff))
                && std::fs::remove_file(e.path()).is_ok()
            {
                out.files += 1;
                out.bytes += m.len();
            }
        }
    }
    out
}

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

#[cfg(test)]
mod sweep_tests {
    use super::*;

    #[test]
    fn sweep_takes_only_old_parts() {
        let root = std::env::temp_dir().join(format!("vutric-sweep-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let deep = root.join("Photos").join("Pixel");
        std::fs::create_dir_all(&deep).unwrap();
        std::fs::write(deep.join("a.jpg.vutric-part"), [0u8; 10]).unwrap();
        std::fs::write(deep.join("a.jpg"), [0u8; 10]).unwrap();
        std::fs::write(root.join("b.vutric-part.txt"), b"x").unwrap();
        let now = SystemTime::now();
        assert_eq!(
            sweep_parts(&root, 7, now),
            Swept::default(),
            "fresh parts stay"
        );
        let later = now + Duration::from_secs(8 * 86_400);
        assert_eq!(
            sweep_parts(&root, 7, later),
            Swept {
                files: 1,
                bytes: 10
            }
        );
        assert!(deep.join("a.jpg").exists() && root.join("b.vutric-part.txt").exists());
        std::fs::write(root.join("c.vutric-part"), [0u8; 3]).unwrap();
        assert_eq!(
            sweep_parts(&root, 0, now),
            Swept { files: 1, bytes: 3 },
            "delete now"
        );
        let _ = std::fs::remove_dir_all(root);
    }
}

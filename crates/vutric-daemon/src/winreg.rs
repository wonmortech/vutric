//! The few per-user registry writes the daemon needs (HKCU only, never admin).

use windows::Win32::Foundation::{ERROR_FILE_NOT_FOUND, WIN32_ERROR};
use windows::Win32::System::Registry::{
    HKEY_CURRENT_USER, REG_SZ, RRF_RT_REG_SZ, RegDeleteKeyValueW, RegGetValueW, RegSetKeyValueW,
};
use windows::core::{HSTRING, PCWSTR};

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn check(e: WIN32_ERROR) -> windows::core::Result<()> {
    e.ok()
}

pub fn set_string(key: &str, name: &str, value: &str) -> windows::core::Result<()> {
    let data = wide(value);
    // SAFETY: every pointer is to a live NUL-terminated buffer; the size is in bytes including the NUL.
    unsafe {
        check(RegSetKeyValueW(
            HKEY_CURRENT_USER,
            &HSTRING::from(key),
            &HSTRING::from(name),
            REG_SZ.0,
            Some(data.as_ptr().cast()),
            (data.len() * 2) as u32,
        ))
    }
}

/// `Ok(None)` when the value does not exist.
pub fn get_string(key: &str, name: &str) -> windows::core::Result<Option<String>> {
    let (k, n) = (HSTRING::from(key), HSTRING::from(name));
    let mut bytes = 0u32;
    // SAFETY: a size query with no buffer, then a read into a buffer of exactly that size.
    unsafe {
        let e = RegGetValueW(
            HKEY_CURRENT_USER,
            &k,
            &n,
            RRF_RT_REG_SZ,
            None,
            None,
            Some(&raw mut bytes),
        );
        if e == ERROR_FILE_NOT_FOUND {
            return Ok(None);
        }
        check(e)?;
        let mut buf = vec![0u16; (bytes as usize).div_ceil(2)];
        check(RegGetValueW(
            HKEY_CURRENT_USER,
            &k,
            &n,
            RRF_RT_REG_SZ,
            None,
            Some(buf.as_mut_ptr().cast()),
            Some(&raw mut bytes),
        ))?;
        let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
        Ok(Some(String::from_utf16_lossy(&buf[..end])))
    }
}

/// Deletes a value; a missing value is not an error.
pub fn delete_value(key: &str, name: &str) -> windows::core::Result<()> {
    // SAFETY: NUL-terminated strings that outlive the call.
    let e = unsafe {
        RegDeleteKeyValueW(
            HKEY_CURRENT_USER,
            PCWSTR(wide(key).as_ptr()),
            PCWSTR(wide(name).as_ptr()),
        )
    };
    if e == ERROR_FILE_NOT_FOUND {
        Ok(())
    } else {
        check(e)
    }
}

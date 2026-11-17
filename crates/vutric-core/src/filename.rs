//! Turning a peer-supplied file name into one that is safe to create in the save folder on every desktop OS.
//!
//! The name comes from another device and must be treated as hostile: no path traversal, no device names,
//! no names that Windows Explorer silently mangles.

const MAX_STEM_CHARS: usize = 120;
const MAX_EXT_CHARS: usize = 16;
const FALLBACK: &str = "vutric-file";

const WINDOWS_RESERVED: [&str; 22] = [
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
    "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

/// Returns a bare file name (never a path) that is valid on Windows, Linux and macOS.
#[must_use]
pub fn sanitize(name: &str) -> String {
    let base = name.rsplit(['/', '\\']).next().unwrap_or_default();
    let cleaned: String = base
        .chars()
        .map(|c| match c {
            '<' | '>' | ':' | '"' | '|' | '?' | '*' => '_',
            c if c.is_control() => '_',
            c => c,
        })
        .collect();
    let trimmed = cleaned.trim_matches(|c: char| c == '.' || c.is_whitespace());

    let (stem, ext) = match trimmed.rsplit_once('.') {
        Some((stem, ext)) if !stem.is_empty() && !ext.is_empty() => (stem, Some(ext)),
        _ => (trimmed, None),
    };
    let mut stem: String = stem.chars().take(MAX_STEM_CHARS).collect();
    let stem_trimmed = stem
        .trim_end_matches(|c: char| c == '.' || c.is_whitespace())
        .len();
    stem.truncate(stem_trimmed);
    if stem.is_empty() {
        FALLBACK.clone_into(&mut stem);
    }
    let device = stem
        .split('.')
        .next()
        .unwrap_or_default()
        .to_ascii_uppercase();
    if WINDOWS_RESERVED.contains(&device.as_str()) {
        stem.insert(0, '_');
    }

    match ext {
        Some(ext) => {
            let ext: String = ext.chars().take(MAX_EXT_CHARS).collect();
            format!("{stem}.{ext}")
        }
        None => stem,
    }
}

/// Picks a name that does not collide, by asking `exists` for each candidate:
/// `photo.jpg`, `photo (2).jpg`, `photo (3).jpg`, …
#[must_use]
pub fn unique(name: &str, exists: impl Fn(&str) -> bool) -> String {
    if !exists(name) {
        return name.to_owned();
    }
    let (stem, ext) = match name.rsplit_once('.') {
        Some((stem, ext)) if !stem.is_empty() => (stem, format!(".{ext}")),
        _ => (name, String::new()),
    };
    (2u32..=u32::MAX)
        .map(|n| format!("{stem} ({n}){ext}"))
        .find(|candidate| !exists(candidate))
        .expect("four billion names cannot all be taken")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_paths_and_traversal() {
        assert_eq!(sanitize("../../etc/passwd"), "passwd");
        assert_eq!(sanitize("C:\\Windows\\system32\\evil.dll"), "evil.dll");
        assert_eq!(sanitize(".."), FALLBACK);
        assert_eq!(sanitize(""), FALLBACK);
    }

    #[test]
    fn replaces_characters_windows_forbids() {
        assert_eq!(sanitize("a<b>c:d\"e|f?g*.png"), "a_b_c_d_e_f_g_.png");
        assert_eq!(sanitize("tab\there.txt"), "tab_here.txt");
    }

    #[test]
    fn neutralizes_reserved_device_names() {
        assert_eq!(sanitize("CON"), "_CON");
        assert_eq!(sanitize("nul.txt"), "_nul.txt");
        assert_eq!(sanitize("com1.tar.gz"), "_com1.tar.gz");
        assert_eq!(sanitize("console.log"), "console.log");
    }

    #[test]
    fn trims_trailing_dots_and_spaces() {
        assert_eq!(sanitize("photo.jpg. . "), "photo.jpg");
        assert_eq!(sanitize("  .hidden"), "hidden");
    }

    #[test]
    fn keeps_unicode_names() {
        assert_eq!(sanitize("Toshkent — kuz 🍂.heic"), "Toshkent — kuz 🍂.heic");
    }

    #[test]
    fn caps_length() {
        let long = format!("{}.{}", "a".repeat(500), "b".repeat(50));
        let out = sanitize(&long);
        assert_eq!(out.chars().count(), MAX_STEM_CHARS + 1 + MAX_EXT_CHARS);
    }

    #[test]
    fn unique_appends_counter() {
        let taken = ["photo.jpg", "photo (2).jpg"];
        assert_eq!(unique("photo.jpg", |c| taken.contains(&c)), "photo (3).jpg");
        assert_eq!(unique("README", |c| c == "README"), "README (2)");
        assert_eq!(unique("new.png", |_| false), "new.png");
    }
}

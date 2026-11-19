//! The UI font for the instant cards on Linux (ADR-0093 §1). The cards are rasterised on the CPU by
//! `vutric_overlay::card` (W2, ADR-0032) from font files; on Linux the desktop's own UI font is the right one,
//! and it differs per distribution. W2's `Fonts::system()` only tries DejaVu, which Fedora Workstation does not
//! install by default; these lists are what it should try (patch in `docs/streams/w4-notes.md`).
//!
//! No fontconfig: one `stat` per candidate is enough for a short list, and the embedded Geist fallback still
//! covers a machine where none exists.

use std::path::{Path, PathBuf};

/// Regular weight, most specific first: GNOME 48+ (Adwaita Sans), Ubuntu 24.04+ (Ubuntu Sans), older Ubuntu,
/// GNOME before 48 (Cantarell), KDE and Fedora KDE (Noto Sans), anything else (DejaVu).
pub const REGULAR: &[&str] = &[
    "/usr/share/fonts/adwaita-sans-fonts/AdwaitaSans-Regular.ttf",
    "/usr/share/fonts/truetype/adwaita-sans/AdwaitaSans-Regular.ttf",
    "/usr/share/fonts/truetype/ubuntu/UbuntuSans[wdth,wght].ttf",
    "/usr/share/fonts/truetype/ubuntu/Ubuntu-R.ttf",
    "/usr/share/fonts/cantarell/Cantarell-VF.otf",
    "/usr/share/fonts/opentype/cantarell/Cantarell-VF.otf",
    "/usr/share/fonts/google-noto-vf/NotoSans[wght].ttf",
    "/usr/share/fonts/truetype/noto/NotoSans-Regular.ttf",
    "/usr/share/fonts/noto/NotoSans-Regular.ttf",
    "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
    "/usr/share/fonts/dejavu-sans-fonts/DejaVuSans.ttf",
    "/usr/share/fonts/TTF/DejaVuSans.ttf",
];

/// Bold / semibold for titles. Variable fonts above have no separate bold file; the renderer then uses the
/// regular face for titles too, which is what `Fonts::with(regular, None)` already does.
pub const STRONG: &[&str] = &[
    "/usr/share/fonts/adwaita-sans-fonts/AdwaitaSans-Bold.ttf",
    "/usr/share/fonts/truetype/ubuntu/Ubuntu-B.ttf",
    "/usr/share/fonts/truetype/noto/NotoSans-Bold.ttf",
    "/usr/share/fonts/noto/NotoSans-Bold.ttf",
    "/usr/share/fonts/truetype/dejavu/DejaVuSans-Bold.ttf",
    "/usr/share/fonts/dejavu-sans-fonts/DejaVuSans-Bold.ttf",
    "/usr/share/fonts/TTF/DejaVuSans-Bold.ttf",
];

/// The first candidate that `exists` accepts.
#[must_use]
pub fn first(candidates: &[&str], exists: impl Fn(&Path) -> bool) -> Option<PathBuf> {
    candidates
        .iter()
        .map(Path::new)
        .find(|p| exists(p))
        .map(Path::to_path_buf)
}

/// `(regular, strong)` font files on this machine; either may be `None`.
#[must_use]
pub fn ui_fonts() -> (Option<PathBuf>, Option<PathBuf>) {
    let exists = |p: &Path| p.is_file();
    (first(REGULAR, exists), first(STRONG, exists))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_most_specific_font_wins() {
        let fedora = |p: &Path| {
            p.starts_with("/usr/share/fonts/adwaita-sans-fonts")
                || p.starts_with("/usr/share/fonts/dejavu-sans-fonts")
        };
        assert_eq!(
            first(REGULAR, fedora),
            Some(PathBuf::from(
                "/usr/share/fonts/adwaita-sans-fonts/AdwaitaSans-Regular.ttf"
            ))
        );
        let old_ubuntu = |p: &Path| p.starts_with("/usr/share/fonts/truetype/ubuntu/Ubuntu-");
        assert_eq!(
            first(REGULAR, old_ubuntu),
            Some(PathBuf::from(
                "/usr/share/fonts/truetype/ubuntu/Ubuntu-R.ttf"
            ))
        );
        assert_eq!(first(STRONG, |_| false), None);
    }

    #[test]
    fn every_candidate_is_absolute_and_a_font_file() {
        for p in REGULAR.iter().chain(STRONG) {
            assert!(p.starts_with("/usr/share/fonts/"), "{p}");
            assert!(p.ends_with(".ttf") || p.ends_with(".otf"), "{p}");
        }
    }
}

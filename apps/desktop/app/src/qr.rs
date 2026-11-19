//! The pairing QR as pixels: black modules on white with the four-module quiet zone scanners expect.

/// Pixels per module and quiet-zone width in modules.
const SCALE: usize = 8;
const QUIET: usize = 4;

/// The QR of `text` as RGBA rows and its side in pixels.
#[must_use]
pub fn rgba(text: &str) -> Option<(Vec<u8>, u32)> {
    let q = qrcode::QrCode::new(text.as_bytes()).ok()?;
    let w = q.width();
    let dark: Vec<bool> = q
        .to_colors()
        .into_iter()
        .map(|c| c == qrcode::Color::Dark)
        .collect();
    Some(paint(&dark, w))
}

fn paint(dark: &[bool], w: usize) -> (Vec<u8>, u32) {
    let side = (w + 2 * QUIET) * SCALE;
    let mut out = vec![255u8; side * side * 4];
    for y in 0..side {
        for x in 0..side {
            let (mx, my) = (x / SCALE, y / SCALE);
            let on = (QUIET..w + QUIET).contains(&mx)
                && (QUIET..w + QUIET).contains(&my)
                && dark[(my - QUIET) * w + (mx - QUIET)];
            if on {
                let i = (y * side + x) * 4;
                out[i..i + 3].fill(0);
            }
        }
    }
    (out, side as u32)
}

/// The QR as a Slint image.
#[must_use]
pub fn image(text: &str) -> Option<slint::Image> {
    let (px, side) = rgba(text)?;
    let buf = slint::SharedPixelBuffer::<slint::Rgba8Pixel>::clone_from_slice(&px, side, side);
    Some(slint::Image::from_rgba8(buf))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finder_corner_is_dark_inside_a_white_quiet_zone() {
        let (px, side) = rgba("vutric://pair?v=1&s=00").unwrap();
        let at = |x: usize, y: usize| px[(y * side as usize + x) * 4];
        assert_eq!(at(0, 0), 255, "quiet zone");
        let first = QUIET * SCALE;
        assert_eq!(at(first, first), 0, "top-left finder pattern");
        assert_eq!(px.len(), (side as usize).pow(2) * 4);
    }
}

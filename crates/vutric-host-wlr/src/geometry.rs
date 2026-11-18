//! Pixel bookkeeping between the card raster (buffer pixels) and Wayland's logical coordinates.

/// Gap between a card and the screen edges, in logical pixels (the Windows card uses the same 16 px).
pub const CARD_MARGIN: i32 = 16;

/// The integer buffer scale for an output scale, clamped to what we rasterise for (1–4).
#[must_use]
pub fn buffer_scale(output_scale: i32) -> i32 {
    output_scale.clamp(1, 4)
}

/// The logical size of a buffer of `px` pixels at `scale`, rounded up.
#[must_use]
pub fn logical(px: u32, scale: i32) -> u32 {
    let s = buffer_scale(scale).cast_unsigned();
    px.div_ceil(s)
}

/// `wl_surface.set_buffer_scale` needs buffer sizes that are multiples of the scale. Pads a premultiplied
/// BGRA raster with transparent pixels on the right and bottom; returns the raster unchanged when it fits.
#[must_use]
pub fn pad_to_scale(bgra: &[u8], width: u32, height: u32, scale: i32) -> (Vec<u8>, u32, u32) {
    let s = buffer_scale(scale).cast_unsigned();
    let (w, h) = (width.div_ceil(s) * s, height.div_ceil(s) * s);
    assert_eq!(bgra.len(), (width * height * 4) as usize, "raster size");
    if (w, h) == (width, height) {
        return (bgra.to_vec(), w, h);
    }
    let mut out = vec![0u8; (w * h * 4) as usize];
    let row = (width * 4) as usize;
    for y in 0..height as usize {
        let dst = y * (w * 4) as usize;
        out[dst..dst + row].copy_from_slice(&bgra[y * row..(y + 1) * row]);
    }
    (out, w, h)
}

/// A pointer position in surface-local logical coordinates, as buffer pixels (what the card hit rects use).
#[must_use]
pub fn to_buffer(x: f64, y: f64, scale: i32) -> (f32, f32) {
    let s = f64::from(buffer_scale(scale));
    ((x * s) as f32, (y * s) as f32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scale_is_clamped() {
        assert_eq!(buffer_scale(0), 1);
        assert_eq!(buffer_scale(2), 2);
        assert_eq!(buffer_scale(9), 4);
    }

    #[test]
    fn logical_rounds_up() {
        assert_eq!(logical(400, 1), 400);
        assert_eq!(logical(801, 2), 401);
        assert_eq!(logical(600, 3), 200);
    }

    #[test]
    fn padding_keeps_pixels_and_adds_transparency() {
        let src: Vec<u8> = (0..3 * 3 * 4).map(|i| i as u8).collect();
        let (out, w, h) = pad_to_scale(&src, 3, 3, 2);
        assert_eq!((w, h), (4, 4));
        assert_eq!(&out[0..12], &src[0..12]);
        assert_eq!(&out[12..16], &[0, 0, 0, 0]);
        assert_eq!(&out[16..28], &src[12..24]);
        assert!(out[48..].iter().all(|b| *b == 0));
        let (same, w1, h1) = pad_to_scale(&src, 3, 3, 1);
        assert_eq!((same.as_slice(), w1, h1), (src.as_slice(), 3, 3));
    }

    #[test]
    fn pointer_maps_to_buffer_pixels() {
        assert_eq!(to_buffer(10.5, 20.0, 2), (21.0, 40.0));
        assert_eq!(to_buffer(10.5, 20.0, 1), (10.5, 20.0));
    }
}

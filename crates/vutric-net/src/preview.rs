//! The photo preview that rides in `FlingStart`, made and opened the same way on every device.
//!
//! - [`encode`] turns any picked image into a JPEG with the longest side at most [`MAX_SIDE`], upright (EXIF
//!   orientation applied), and small enough for [`TARGET_BYTES`] (quality steps down until it fits).
//! - [`decode`] opens a preview that came from the network with hard limits, so a few hostile bytes cannot
//!   claim a giant image and exhaust memory.

use std::io::Cursor;

use anyhow::{Context as _, Result, bail, ensure};
use image::{DynamicImage, ImageDecoder as _, ImageFormat, ImageReader, Limits};
use vutric_core::message::MAX_PREVIEW_BYTES;

/// Longest side of a preview, in pixels.
pub const MAX_SIDE: u32 = 512;

/// Size a preview aims for; well under [`MAX_PREVIEW_BYTES`] so the frame stays one fast write.
pub const TARGET_BYTES: usize = 64 * 1024;

/// A preview ready to send.
#[derive(Debug, Clone)]
pub struct Encoded {
    pub jpeg: Vec<u8>,
    pub width: u16,
    pub height: u16,
}

/// Limits for images from the user's own device (a phone camera photo can be 50 MP).
fn local_limits() -> Limits {
    let mut l = Limits::default();
    l.max_image_width = Some(16_384);
    l.max_image_height = Some(16_384);
    l.max_alloc = Some(512 * 1024 * 1024);
    l
}

/// Limits for previews from another device: nothing bigger than a preview may even be allocated.
fn network_limits() -> Limits {
    let mut l = Limits::default();
    l.max_image_width = Some(MAX_SIDE * 2);
    l.max_image_height = Some(MAX_SIDE * 2);
    l.max_alloc = Some(16 * 1024 * 1024);
    l
}

/// Decodes `bytes` upright: the EXIF orientation is applied, as every photo viewer does.
fn open_upright(bytes: &[u8], limits: Limits) -> Result<DynamicImage> {
    let mut reader = ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .context("read image")?;
    reader.limits(limits);
    let mut decoder = reader
        .into_decoder()
        .context("not a JPEG, PNG or WebP image")?;
    let orientation = decoder.orientation().ok();
    let mut img = DynamicImage::from_decoder(decoder).context("decode image")?;
    if let Some(o) = orientation {
        img.apply_orientation(o);
    }
    Ok(img)
}

/// Any picked image (JPEG, PNG or WebP) → the preview to send.
pub fn encode(bytes: &[u8]) -> Result<Encoded> {
    let img = open_upright(bytes, local_limits())?
        .thumbnail(MAX_SIDE, MAX_SIDE)
        .to_rgb8();
    let (width, height) = (u16::try_from(img.width())?, u16::try_from(img.height())?);
    let mut last = 0;
    for quality in [80u8, 70, 60, 50, 40] {
        let mut jpeg = Vec::new();
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpeg, quality)
            .encode_image(&img)?;
        if jpeg.len() <= TARGET_BYTES {
            return Ok(Encoded {
                jpeg,
                width,
                height,
            });
        }
        last = jpeg.len();
    }
    bail!("the preview stays {last} bytes even at the lowest quality")
}

/// A preview from the network → pixels, or an error if it is not a sane preview. `declared` is the size the
/// sender stated; it must match what the bytes say.
pub fn decode(bytes: &[u8], declared: (u16, u16)) -> Result<image::RgbaImage> {
    ensure!(
        bytes.len() <= MAX_PREVIEW_BYTES,
        "preview of {} bytes",
        bytes.len()
    );
    ensure!(
        image::guess_format(bytes).ok() == Some(ImageFormat::Jpeg),
        "preview is not a JPEG"
    );
    let img = open_upright(bytes, network_limits())?;
    let (w, h) = (img.width(), img.height());
    ensure!(
        w.max(h) <= MAX_SIDE * 2,
        "preview is {w}x{h}, larger than a preview"
    );
    let (dw, dh) = (u32::from(declared.0), u32::from(declared.1));
    ensure!(
        (w, h) == (dw, dh) || (w, h) == (dh, dw),
        "preview is {w}x{h} but was announced as {dw}x{dh}"
    );
    Ok(img.to_rgba8())
}

/// Checks a preview without keeping the pixels (the phone hands the JPEG itself to Flutter).
pub fn check(bytes: &[u8], declared: (u16, u16)) -> Result<()> {
    decode(bytes, declared).map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn jpeg_of(w: u32, h: u32, noise: bool) -> Vec<u8> {
        let img = image::RgbImage::from_fn(w, h, |x, y| {
            if noise {
                let v = (x.wrapping_mul(2_654_435_761) ^ y.wrapping_mul(40_503)).to_le_bytes();
                image::Rgb([v[0], v[1], v[2]])
            } else {
                image::Rgb([(x % 256) as u8, (y % 256) as u8, 90])
            }
        });
        let mut out = Vec::new();
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 95)
            .encode_image(&img)
            .expect("encode");
        out
    }

    #[test]
    fn encode_shrinks_and_fits_even_noise() {
        let e = encode(&jpeg_of(2000, 1500, true)).expect("encode");
        assert_eq!((e.width, e.height), (512, 384));
        assert!(e.jpeg.len() <= TARGET_BYTES, "{} bytes", e.jpeg.len());
        decode(&e.jpeg, (e.width, e.height)).expect("round trip");
    }

    #[test]
    fn decode_rejects_lies_and_giants() {
        let ok = encode(&jpeg_of(640, 480, false)).expect("encode");
        assert!(decode(&ok.jpeg, (100, 100)).is_err(), "size mismatch");
        let giant = jpeg_of(1400, 200, false);
        assert!(decode(&giant, (1400, 200)).is_err(), "too large");
        assert!(
            decode(b"\x89PNG\r\n\x1a\nnot really", (1, 1)).is_err(),
            "not JPEG"
        );
        assert!(
            decode(&[0xff, 0xd8, 0xff, 0, 0, 0], (1, 1)).is_err(),
            "truncated"
        );
    }
}

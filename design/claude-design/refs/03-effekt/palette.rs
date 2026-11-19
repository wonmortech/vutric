/// Effect colours in linear RGB, alpha 1. Constructed from sRGB bytes, the way designers write them.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Palette {
    pub rim_a: [f32; 4],
    pub rim_b: [f32; 4],
    pub depth: [f32; 4],
}

impl Palette {
    /// Vutric default (design system v0.1): Signal 500 `#2F6BFF` → Glow 200 `#F4D9A6`, over a Night 950 `#040C29` portal.
    #[must_use]
    pub fn vutric() -> Self {
        Self {
            rim_a: srgb(0x2F, 0x6B, 0xFF),
            rim_b: srgb(0xF4, 0xD9, 0xA6),
            depth: srgb(0x04, 0x0C, 0x29),
        }
    }
}

impl Default for Palette {
    fn default() -> Self {
        Self::vutric()
    }
}

/// sRGB bytes → linear RGBA with alpha 1.
#[must_use]
pub fn srgb(r: u8, g: u8, b: u8) -> [f32; 4] {
    let channel = |v: u8| srgb_to_linear(f32::from(v) / 255.0);
    [channel(r), channel(g), channel(b), 1.0]
}

#[must_use]
pub fn srgb_to_linear(c: f32) -> f32 {
    if c <= 0.040_45 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bytes_to_linear() {
        assert_eq!(srgb(0, 0, 0), [0.0, 0.0, 0.0, 1.0]);
        assert_eq!(srgb(255, 255, 255), [1.0, 1.0, 1.0, 1.0]);
        let mid = srgb(128, 128, 128)[0];
        assert!((mid - 0.2158).abs() < 1e-3, "{mid}");
    }
}

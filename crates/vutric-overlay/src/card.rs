//! The instant cards (ADR-0009 §1): "Qabul qilasizmi?", the progress ring, "Tushdi" and the pairing QR.
//!
//! A card is rasterised on the CPU into premultiplied BGRA (tiny-skia shapes, fontdue glyphs) and handed to the
//! host, which shows it in its own small layered window: that window takes clicks without ever taking focus, while
//! the full-screen effect surface stays click-through. No GPU work, so a card costs nothing while hidden and shows
//! in about a millisecond.

// Geometry and pixel blending read best with x/y/w/h/r/g/b/a names.
#![allow(clippy::many_single_char_names)]

use tiny_skia::{
    Color, FillRule, LineCap, Paint, PathBuilder, Pixmap, PremultipliedColorU8, Rect, Stroke,
    Transform,
};

use ab_glyph::{Font as _, FontArc, PxScale, ScaleFont as _, point};

use crate::palette::Palette;

/// What a card shows. Strings arrive already translated.
#[derive(Debug, Clone, PartialEq)]
pub enum Card {
    Offer {
        title: String,
        body: String,
        accept: String,
        decline: String,
    },
    Progress {
        title: String,
        body: String,
        /// 0..=1; `None` = waiting, the ring spins.
        fraction: Option<f32>,
        /// Seconds, drives the spinner.
        t: f32,
    },
    Landed {
        title: String,
        body: String,
    },
    Pair {
        title: String,
        body: String,
        close: String,
        /// QR modules, row-major, `true` = dark; `qr_width` per row.
        qr: Vec<bool>,
        qr_width: usize,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Button {
    Accept,
    Decline,
    Close,
}

/// A rasterised card: premultiplied BGRA rows, plus the clickable rectangles in the same pixel space.
#[derive(Debug, Clone)]
pub struct Rendered {
    pub width: u32,
    pub height: u32,
    pub bgra: Vec<u8>,
    pub buttons: Vec<(Button, [f32; 4])>,
}

impl Rendered {
    /// The button under a point (window pixels), if any.
    #[must_use]
    pub fn hit(&self, x: f32, y: f32) -> Option<Button> {
        self.buttons
            .iter()
            .find(|(_, r)| x >= r[0] && x < r[2] && y >= r[1] && y < r[3])
            .map(|(b, _)| *b)
    }
}

/// Fonts tried in order for every character: the system UI font first (it covers every script a device name may
/// use), then the embedded Geist subset, so a card can always be drawn.
///
/// `ab_glyph` parses only the table directory up front and outlines a glyph when it is first drawn, so loading
/// costs a file read (about 1 ms) instead of parsing every glyph of Segoe UI (89 ms with fontdue, measured on the
/// first card).
#[derive(Debug, Clone)]
pub struct Fonts {
    regular: Vec<FontArc>,
    strong: Vec<FontArc>,
}

/// Geist Regular (SIL OFL 1.1, `fonts/OFL.txt`): Latin only, the fallback when no system font is found.
pub const GEIST: &[u8] = include_bytes!("../fonts/Geist.ttf");

impl Fonts {
    /// Loads the system UI font when present, always followed by the embedded fallback.
    #[must_use]
    pub fn system() -> Self {
        let load = |paths: &[&str]| {
            paths
                .iter()
                .filter_map(|p| std::fs::read(p).ok())
                .find_map(|b| FontArc::try_from_vec(b).ok())
        };
        let (regular, strong) = if cfg!(windows) {
            let dir = std::env::var("WINDIR").unwrap_or_else(|_| r"C:\Windows".into());
            (
                load(&[&format!(r"{dir}\Fonts\segoeui.ttf")]),
                load(&[&format!(r"{dir}\Fonts\seguisb.ttf")]),
            )
        } else {
            let paths = [
                "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
                "/usr/share/fonts/TTF/DejaVuSans.ttf",
            ];
            (
                load(&paths),
                load(&["/usr/share/fonts/truetype/dejavu/DejaVuSans-Bold.ttf"]),
            )
        };
        Self::with(regular, strong)
    }

    /// The embedded fallback only (tests, and machines with no usable system font).
    #[must_use]
    pub fn embedded() -> Self {
        Self::with(None, None)
    }

    fn with(regular: Option<FontArc>, strong: Option<FontArc>) -> Self {
        let geist = FontArc::try_from_slice(GEIST).expect("embedded font");
        let mut r: Vec<_> = regular.into_iter().collect();
        r.push(geist);
        let mut s: Vec<_> = strong.into_iter().collect();
        s.extend(r.iter().cloned());
        Self {
            regular: r,
            strong: s,
        }
    }
}

/// Sizes in logical pixels; everything is multiplied by the monitor scale.
const WIDTH: f32 = 360.0;
const PAD: f32 = 18.0;
const SHADOW: f32 = 20.0;
const RADIUS: f32 = 18.0;

fn rgba(c: [f32; 4], a: f32) -> Color {
    Color::from_rgba(c[0], c[1], c[2], (c[3] * a).clamp(0.0, 1.0)).unwrap_or(Color::TRANSPARENT)
}

fn rounded(x: f32, y: f32, w: f32, h: f32, r: f32) -> Option<tiny_skia::Path> {
    let r = r.min(w / 2.0).min(h / 2.0);
    let mut p = PathBuilder::new();
    p.move_to(x + r, y);
    p.line_to(x + w - r, y);
    p.quad_to(x + w, y, x + w, y + r);
    p.line_to(x + w, y + h - r);
    p.quad_to(x + w, y + h, x + w - r, y + h);
    p.line_to(x + r, y + h);
    p.quad_to(x, y + h, x, y + h - r);
    p.line_to(x, y + r);
    p.quad_to(x, y, x + r, y);
    p.close();
    p.finish()
}

fn fill(pm: &mut Pixmap, path: Option<tiny_skia::Path>, color: Color) {
    if let Some(path) = path {
        let mut paint = Paint::default();
        paint.set_color(color);
        paint.anti_alias = true;
        pm.fill_path(
            &path,
            &paint,
            FillRule::Winding,
            Transform::identity(),
            None,
        );
    }
}

/// The palette stores linear values (for the GPU); the 8-bit card wants sRGB.
fn srgb(c: [f32; 4]) -> [f32; 4] {
    let enc = |v: f32| {
        if v <= 0.003_130_8 {
            v * 12.92
        } else {
            1.055 * v.powf(1.0 / 2.4) - 0.055
        }
    };
    [enc(c[0]), enc(c[1]), enc(c[2]), c[3]]
}

struct Ink<'a> {
    fonts: &'a [FontArc],
    /// Em size in pixels.
    px: f32,
}

impl Ink<'_> {
    fn font_for(&self, ch: char) -> &FontArc {
        self.fonts
            .iter()
            .find(|f| f.glyph_id(ch).0 != 0)
            .unwrap_or(&self.fonts[0])
    }

    /// `ab_glyph` scales by line height; cards are specified in em size like CSS.
    fn scale(&self, f: &FontArc) -> PxScale {
        let em = f.units_per_em().unwrap_or(1000.0);
        PxScale::from(self.px * f.height_unscaled() / em)
    }

    fn advance(&self, ch: char) -> f32 {
        let f = self.font_for(ch);
        f.as_scaled(self.scale(f)).h_advance(f.glyph_id(ch))
    }

    fn width(&self, s: &str) -> f32 {
        s.chars().map(|c| self.advance(c)).sum()
    }

    /// `s` shortened with "…" to fit `max`.
    fn elide(&self, s: &str, max: f32) -> String {
        if self.width(s) <= max {
            return s.to_owned();
        }
        let ell = self.width("…");
        let mut out = String::new();
        let mut w = 0.0;
        for c in s.chars() {
            let a = self.advance(c);
            if w + a + ell > max {
                break;
            }
            w += a;
            out.push(c);
        }
        out.push('…');
        out
    }

    /// Draws `s` with its baseline at `y`; returns the width drawn.
    fn draw(&self, pm: &mut Pixmap, s: &str, x: f32, y: f32, color: [f32; 4]) -> f32 {
        let [r, g, b, a] = srgb(color);
        let (w, h) = (pm.width() as usize, pm.height() as usize);
        let mut pen = x;
        for ch in s.chars().filter(|c| !c.is_control()) {
            let font = self.font_for(ch);
            let scale = self.scale(font);
            let id = font.glyph_id(ch);
            let glyph = id.with_scale_and_position(scale, point(pen.round(), y.round()));
            if let Some(outline) = font.outline_glyph(glyph) {
                let bounds = outline.px_bounds();
                let data = pm.pixels_mut();
                outline.draw(|gx, gy, cov| {
                    let (fx, fy) = (bounds.min.x + gx as f32, bounds.min.y + gy as f32);
                    if fx < 0.0 || fy < 0.0 || fx >= w as f32 || fy >= h as f32 {
                        return;
                    }
                    let k = cov.clamp(0.0, 1.0) * a;
                    if k <= 0.0 {
                        return;
                    }
                    let i = fy as usize * w + fx as usize;
                    let d = data[i];
                    let mix = |src: f32, dst: u8| {
                        (src * k * 255.0 + f32::from(dst) * (1.0 - k)).round() as u8
                    };
                    let out_a = (k * 255.0 + f32::from(d.alpha()) * (1.0 - k)).round() as u8;
                    let (cr, cg, cb) = (mix(r, d.red()), mix(g, d.green()), mix(b, d.blue()));
                    data[i] = PremultipliedColorU8::from_rgba(
                        cr.min(out_a),
                        cg.min(out_a),
                        cb.min(out_a),
                        out_a,
                    )
                    .unwrap_or(d);
                });
            }
            pen += font.as_scaled(scale).h_advance(id);
        }
        pen - x
    }
}

/// Text colours (linear) used on the night card.
const TEXT: [f32; 4] = [0.85, 0.88, 0.96, 1.0];
const MUTED: [f32; 4] = [0.33, 0.38, 0.6, 1.0];
const LINE: [f32; 4] = [0.012, 0.024, 0.13, 1.0];

/// Rasterises `card` at `scale` (monitor DPI / 96).
#[must_use]
#[allow(clippy::too_many_lines)] // one card layout per arm, read top to bottom
pub fn render(card: &Card, fonts: &Fonts, palette: &Palette, scale: f32) -> Rendered {
    let s = scale.clamp(0.5, 4.0);
    let inner_h = match card {
        Card::Offer { .. } => 136.0,
        Card::Progress { .. } | Card::Landed { .. } => 84.0,
        Card::Pair { .. } => 358.0,
    };
    let (w, h) = (WIDTH + SHADOW * 2.0, inner_h + SHADOW * 2.0);
    let (pw, ph) = ((w * s).ceil() as u32, (h * s).ceil() as u32);
    let mut pm = Pixmap::new(pw, ph).expect("non-zero card size");
    let (x0, y0) = (SHADOW * s, SHADOW * s);
    let (cw, ch) = (WIDTH * s, inner_h * s);

    // Soft shadow: stacked rounded rects, widest and faintest first.
    for i in 0..8 {
        let k = i as f32 / 8.0;
        let grow = (1.0 - k) * SHADOW * s * 0.7;
        fill(
            &mut pm,
            rounded(
                x0 - grow,
                y0 - grow + 4.0 * s,
                cw + grow * 2.0,
                ch + grow * 2.0,
                (RADIUS + grow / s) * s,
            ),
            Color::from_rgba8(0, 0, 0, (2.0 + 6.0 * k) as u8),
        );
    }
    fill(
        &mut pm,
        rounded(x0, y0, cw, ch, RADIUS * s),
        rgba(srgb(LINE), 1.0),
    );
    fill(
        &mut pm,
        rounded(
            x0 + s,
            y0 + s,
            cw - 2.0 * s,
            ch - 2.0 * s,
            (RADIUS - 1.0) * s,
        ),
        rgba(srgb(palette.depth), 0.97),
    );

    let title = Ink {
        fonts: &fonts.strong,
        px: 15.0 * s,
    };
    let body = Ink {
        fonts: &fonts.regular,
        px: 13.0 * s,
    };
    let label = Ink {
        fonts: &fonts.strong,
        px: 13.5 * s,
    };
    let text_x = x0 + (PAD + 44.0) * s;
    let text_w = cw - (PAD * 2.0 + 44.0) * s;
    let mut buttons = Vec::new();

    let portal = |pm: &mut Pixmap, cx: f32, cy: f32| {
        let mut pb = PathBuilder::new();
        if let Some(r) = Rect::from_xywh(cx - 8.0 * s, cy - 16.0 * s, 16.0 * s, 32.0 * s) {
            pb.push_oval(r);
        }
        if let Some(path) = pb.finish() {
            for (wid, a, c) in [(7.0, 0.25, palette.rim_a), (2.2, 1.0, palette.rim_b)] {
                let mut paint = Paint::default();
                paint.set_color(rgba(srgb(c), a));
                paint.anti_alias = true;
                let stroke = Stroke {
                    width: wid * s,
                    ..Stroke::default()
                };
                pm.stroke_path(&path, &paint, &stroke, Transform::identity(), None);
            }
        }
    };

    match card {
        Card::Offer {
            title: t,
            body: b,
            accept,
            decline,
        } => {
            portal(&mut pm, x0 + (PAD + 16.0) * s, y0 + 38.0 * s);
            title.draw(
                &mut pm,
                &title.elide(t, text_w),
                text_x,
                y0 + 34.0 * s,
                TEXT,
            );
            body.draw(
                &mut pm,
                &body.elide(b, text_w),
                text_x,
                y0 + 56.0 * s,
                MUTED,
            );
            let bh = 38.0 * s;
            let by = y0 + ch - PAD * s - bh;
            let bw = (cw - PAD * 2.0 * s - 10.0 * s) / 2.0;
            let dx = x0 + PAD * s;
            let ax = dx + bw + 10.0 * s;
            fill(
                &mut pm,
                rounded(dx, by, bw, bh, 11.0 * s),
                rgba(srgb(MUTED), 0.55),
            );
            fill(
                &mut pm,
                rounded(dx + s, by + s, bw - 2.0 * s, bh - 2.0 * s, 10.0 * s),
                rgba(srgb(palette.depth), 1.0),
            );
            fill(
                &mut pm,
                rounded(ax, by, bw, bh, 11.0 * s),
                rgba(srgb(palette.rim_a), 1.0),
            );
            for (x, text, color) in [(dx, decline, TEXT), (ax, accept, [1.0, 1.0, 1.0, 1.0])] {
                let text = label.elide(text, bw - 16.0 * s);
                let tw = label.width(&text);
                label.draw(
                    &mut pm,
                    &text,
                    x + (bw - tw) / 2.0,
                    by + bh / 2.0 + 5.0 * s,
                    color,
                );
            }
            buttons.push((Button::Decline, [dx, by, dx + bw, by + bh]));
            buttons.push((Button::Accept, [ax, by, ax + bw, by + bh]));
        }
        Card::Progress {
            title: t,
            body: b,
            fraction,
            t: time,
        } => {
            let (cx, cy, r) = (x0 + (PAD + 16.0) * s, y0 + ch / 2.0, 15.0 * s);
            let arc = |pm: &mut Pixmap, from: f32, sweep: f32, color: Color| {
                let mut pb = PathBuilder::new();
                let steps = (sweep.abs() * 24.0).ceil().max(2.0) as usize;
                for i in 0..=steps {
                    let a = from + sweep * i as f32 / steps as f32;
                    let (x, y) = (cx + r * a.cos(), cy + r * a.sin());
                    if i == 0 {
                        pb.move_to(x, y);
                    } else {
                        pb.line_to(x, y);
                    }
                }
                if let Some(path) = pb.finish() {
                    let mut paint = Paint::default();
                    paint.set_color(color);
                    paint.anti_alias = true;
                    let stroke = Stroke {
                        width: 3.5 * s,
                        line_cap: LineCap::Round,
                        ..Stroke::default()
                    };
                    pm.stroke_path(&path, &paint, &stroke, Transform::identity(), None);
                }
            };
            let tau = std::f32::consts::TAU;
            arc(&mut pm, 0.0, tau, rgba(srgb(MUTED), 0.35));
            let (from, sweep) = match fraction {
                Some(f) => (-tau / 4.0, tau * f.clamp(0.0, 1.0)),
                None => (*time * 5.0, tau * 0.28),
            };
            if sweep > 0.0 {
                arc(&mut pm, from, sweep, rgba(srgb(palette.rim_a), 1.0));
                let end = from + sweep;
                let mut pb = PathBuilder::new();
                pb.push_circle(cx + r * end.cos(), cy + r * end.sin(), 2.4 * s);
                fill(&mut pm, pb.finish(), rgba(srgb(palette.rim_b), 1.0));
            }
            title.draw(
                &mut pm,
                &title.elide(t, text_w),
                text_x,
                y0 + 38.0 * s,
                TEXT,
            );
            body.draw(
                &mut pm,
                &body.elide(b, text_w),
                text_x,
                y0 + 60.0 * s,
                MUTED,
            );
        }
        Card::Landed { title: t, body: b } => {
            portal(&mut pm, x0 + (PAD + 16.0) * s, y0 + ch / 2.0);
            title.draw(
                &mut pm,
                &title.elide(t, text_w),
                text_x,
                y0 + 38.0 * s,
                TEXT,
            );
            body.draw(
                &mut pm,
                &body.elide(b, text_w),
                text_x,
                y0 + 60.0 * s,
                MUTED,
            );
        }
        Card::Pair {
            title: t,
            body: b,
            close,
            qr,
            qr_width,
        } => {
            let full_w = cw - PAD * 2.0 * s;
            title.draw(
                &mut pm,
                &title.elide(t, full_w),
                x0 + PAD * s,
                y0 + 36.0 * s,
                TEXT,
            );
            body.draw(
                &mut pm,
                &body.elide(b, full_w),
                x0 + PAD * s,
                y0 + 58.0 * s,
                MUTED,
            );
            let side = 196.0 * s;
            let (qx, qy) = (x0 + (cw - side) / 2.0, y0 + 88.0 * s);
            fill(
                &mut pm,
                rounded(
                    qx - 10.0 * s,
                    qy - 10.0 * s,
                    side + 20.0 * s,
                    side + 20.0 * s,
                    12.0 * s,
                ),
                Color::WHITE,
            );
            if *qr_width > 0 && qr.len() == qr_width * qr_width {
                // Whole-pixel modules, centred in the white square: crisp for the phone camera and written straight
                // into the buffer (anti-aliased paths cost ~6 ms for ~1000 modules).
                let m = ((side / *qr_width as f32).floor() as usize).max(1);
                let span = m * qr_width;
                let ox = (qx + (side - span as f32) / 2.0).round().max(0.0) as usize;
                let oy = (qy + (side - span as f32) / 2.0).round().max(0.0) as usize;
                let stride = pm.width() as usize;
                let rows = pm.height() as usize;
                let ink = PremultipliedColorU8::from_rgba(4, 12, 41, 255).expect("opaque");
                let data = pm.pixels_mut();
                for (i, _) in qr.iter().enumerate().filter(|(_, d)| **d) {
                    let (mx, my) = (ox + (i % qr_width) * m, oy + (i / qr_width) * m);
                    for y in my..(my + m).min(rows) {
                        let row = y * stride;
                        data[row + mx.min(stride)..row + (mx + m).min(stride)].fill(ink);
                    }
                }
            }
            let (bw, bh) = (cw - PAD * 2.0 * s, 36.0 * s);
            let (bx, by) = (x0 + PAD * s, y0 + ch - PAD * s - bh);
            fill(
                &mut pm,
                rounded(bx, by, bw, bh, 11.0 * s),
                rgba(srgb(MUTED), 0.4),
            );
            let tw = label.width(close);
            label.draw(
                &mut pm,
                close,
                bx + (bw - tw) / 2.0,
                by + bh / 2.0 + 5.0 * s,
                TEXT,
            );
            buttons.push((Button::Close, [bx, by, bx + bw, by + bh]));
        }
    }

    let mut bgra = pm.take();
    for px in bgra.chunks_exact_mut(4) {
        px.swap(0, 2);
    }
    Rendered {
        width: pw,
        height: ph,
        bgra,
        buttons,
    }
}

/// QR modules for `text`, row-major (`true` = dark), with the module count per row.
#[must_use]
pub fn qr(text: &str) -> Option<(Vec<bool>, usize)> {
    let q = qrcode::QrCode::new(text.as_bytes()).ok()?;
    let w = q.width();
    Some((
        q.to_colors()
            .into_iter()
            .map(|c| c == qrcode::Color::Dark)
            .collect(),
        w,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn offer(device: &str) -> Card {
        Card::Offer {
            title: "Qabul qilasizmi?".into(),
            body: format!("{device}: 3 ta fayl, 12,4 MB"),
            accept: "Qabul qilish".into(),
            decline: "Rad etish".into(),
        }
    }

    fn px(r: &Rendered, x: f32, y: f32) -> [u8; 4] {
        let i = ((y as u32 * r.width + x as u32) * 4) as usize;
        [r.bgra[i], r.bgra[i + 1], r.bgra[i + 2], r.bgra[i + 3]]
    }

    #[test]
    fn offer_card_has_two_buttons_inside_the_card() {
        let r = render(
            &offer("Pixel 9"),
            &Fonts::embedded(),
            &Palette::vutric(),
            1.5,
        );
        assert_eq!((r.width, r.height), (600, 264));
        assert_eq!(r.bgra.len(), (r.width * r.height * 4) as usize);
        assert_eq!(r.buttons.len(), 2);
        for (_, b) in &r.buttons {
            assert!(
                b[0] >= SHADOW * 1.5 && b[2] <= r.width as f32 - SHADOW * 1.5,
                "{b:?}"
            );
            assert!(b[3] <= r.height as f32 - SHADOW * 1.5);
        }
        let (_, a) = r.buttons[1];
        let c = px(&r, a[0] + 8.0, f32::midpoint(a[1], a[3]));
        assert!(
            c[0] > 200 && c[3] == 255,
            "accept is signal blue (BGRA {c:?})"
        );
        assert_eq!(
            r.hit(f32::midpoint(a[0], a[2]), f32::midpoint(a[1], a[3])),
            Some(Button::Accept)
        );
        assert_eq!(r.hit(1.0, 1.0), None);
        assert_eq!(px(&r, 0.0, 0.0)[3], 0, "the corner is transparent");
    }

    #[test]
    fn premultiplied_alpha_holds_everywhere() {
        let r = render(&offer("A"), &Fonts::embedded(), &Palette::vutric(), 1.0);
        for p in r.bgra.chunks_exact(4) {
            assert!(p[0] <= p[3] && p[1] <= p[3] && p[2] <= p[3], "{p:?}");
        }
    }

    #[test]
    fn text_is_drawn_and_long_names_are_elided() {
        let fonts = Fonts::embedded();
        let ink = Ink {
            fonts: &fonts.regular,
            px: 13.0,
        };
        let long = "Juda uzun nomli telefon ".repeat(10);
        let e = ink.elide(&long, 200.0);
        assert!(e.ends_with('…') && ink.width(&e) <= 200.0, "{e}");
        assert_eq!(ink.elide("Pixel", 200.0), "Pixel");

        let bright = |c: &Card| {
            let r = render(c, &fonts, &Palette::vutric(), 1.0);
            r.bgra
                .chunks_exact(4)
                .filter(|p| p[2] > 150 && p[1] > 150)
                .count()
        };
        let with_text = bright(&offer("Pixel"));
        let no_text = bright(&Card::Offer {
            title: String::new(),
            body: String::new(),
            accept: String::new(),
            decline: String::new(),
        });
        assert!(with_text > no_text + 100, "{with_text} vs {no_text}");
    }

    #[test]
    fn progress_ring_grows_with_the_fraction() {
        let fonts = Fonts::embedded();
        let lit = |f: Option<f32>| {
            let c = Card::Progress {
                title: "Qabul qilinmoqda".into(),
                body: "42%".into(),
                fraction: f,
                t: 0.0,
            };
            let r = render(&c, &fonts, &Palette::vutric(), 1.0);
            r.bgra
                .chunks_exact(4)
                .filter(|p| p[0] > 180 && p[2] < 120)
                .count()
        };
        let (none, half, full) = (lit(Some(0.0)), lit(Some(0.5)), lit(Some(1.0)));
        assert!(none < half && half < full, "{none} {half} {full}");
        assert!(lit(None) > none, "the waiting spinner is visible");
    }

    #[test]
    fn pair_card_draws_the_qr_and_a_close_button() {
        let (code, width) = qr("vutric://pair?x=1").unwrap();
        let r = render(
            &Card::Pair {
                title: "Telefonni ulash".into(),
                body: "QR'ni skanerlang".into(),
                close: "Yopish".into(),
                qr: code,
                qr_width: width,
            },
            &Fonts::embedded(),
            &Palette::vutric(),
            1.0,
        );
        assert_eq!(r.buttons.len(), 1);
        assert_eq!(r.buttons[0].0, Button::Close);
        let dark = r
            .bgra
            .chunks_exact(4)
            .filter(|p| p[3] == 255 && p[0] < 60 && p[2] < 20)
            .count();
        assert!(dark > 2000, "QR modules drawn: {dark}");
    }
}

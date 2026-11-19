//! D2 measurement: how fast each instant card is rasterised and put on screen, and that showing it never moves
//! keyboard focus. Writes one PNG per card next to `--out` for a visual check.
//!
//! ```text
//! cargo run -p vutric-daemon --release --example cards -- --out C:\vutric-w2\cards.json [--runs 200]
//! ```
//!
//! Run it in the interactive session (schtasks /IT) for the focus check to mean anything.

use std::path::PathBuf;
use std::time::Instant;

use vutric_daemon::cards::Deck;
use vutric_daemon::i18n::Lang;
use vutric_overlay::card::{self, Fonts};
use vutric_overlay::palette::Palette;

fn pct(v: &[f64], p: f64) -> f64 {
    let mut v = v.to_vec();
    v.sort_by(f64::total_cmp);
    let i = ((v.len() - 1) as f64 * p).round() as usize;
    (v[i] * 1000.0).round() / 1000.0
}

#[allow(clippy::too_many_lines, clippy::many_single_char_names)] // one linear measurement script
fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    let (mut out, mut runs) = (PathBuf::from("cards.json"), 200usize);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--out" => out = args.next().map_or(out, PathBuf::from),
            "--runs" => runs = args.next().and_then(|r| r.parse().ok()).unwrap_or(runs),
            _ => {}
        }
    }

    let t0 = Instant::now();
    let fonts = Fonts::system();
    let fonts_ms = t0.elapsed().as_secs_f64() * 1000.0;
    let palette = Palette::vutric();
    #[cfg(windows)]
    let scale = vutric_host_win::card::target_scale();
    #[cfg(not(windows))]
    let scale = 1.0;

    let now = Instant::now();
    let device = "Pixel 9 Pro (Aziza)";
    let mut decks = Vec::new();
    let mut d = Deck::new(Lang::Uz);
    d.offer(device, 3, 12_400_000, now);
    decks.push(("offer", d));
    let mut d = Deck::new(Lang::Uz);
    d.progress(5_210_000, 12_400_000);
    decks.push(("progress", d));
    let mut d = Deck::new(Lang::Uz);
    d.received(device, 3, now);
    decks.push(("landed", d));
    let mut d = Deck::new(Lang::Uz);
    d.pair(
        "vutric://pair?v=1&id=k51tq3m0c2x8fz9e7d4a6b1h0g5j3n2p&addr=192.168.1.24:47820&code=73914",
    );
    decks.push(("pair", d));

    #[cfg(windows)]
    let clicked = std::rc::Rc::new(std::cell::Cell::new(None::<(f32, f32)>));
    #[cfg(windows)]
    let window = {
        let c = std::rc::Rc::clone(&clicked);
        vutric_host_win::card::CardWindow::new(move |m| {
            if let vutric_host_win::card::Mouse::Up(x, y) = m {
                c.set(Some((x, y)));
            }
            false
        })
        .map_err(anyhow::Error::msg)?
    };
    #[cfg(windows)]
    let focus_before = vutric_host_win::foreground_window();

    let mut report = Vec::new();
    for (name, deck) in &decks {
        let c = deck.card(card::qr).expect("a card");
        let mut raster = Vec::with_capacity(runs);
        #[cfg_attr(not(windows), allow(unused_mut))]
        let mut show: Vec<f64> = Vec::with_capacity(runs);
        let mut last = None;
        for _ in 0..runs.max(1) {
            let a = Instant::now();
            let r = card::render(&c, &fonts, &palette, scale);
            raster.push(a.elapsed().as_secs_f64() * 1000.0);
            #[cfg(windows)]
            {
                let b = Instant::now();
                let at = vutric_host_win::card::placement(r.width, r.height);
                window
                    .show(r.width, r.height, &r.bgra, at)
                    .map_err(anyhow::Error::msg)?;
                show.push(b.elapsed().as_secs_f64() * 1000.0);
            }
            last = Some(r);
        }
        let r = last.expect("at least one run");
        let mut rgba = r.bgra.clone();
        for p in rgba.chunks_exact_mut(4) {
            p.swap(0, 2);
        }
        let png = out.with_file_name(format!("card-{name}.png"));
        image::RgbaImage::from_raw(r.width, r.height, rgba)
            .expect("size matches")
            .save(&png)?;
        let total: Vec<f64> = raster
            .iter()
            .enumerate()
            .map(|(i, a)| a + show.get(i).copied().unwrap_or(0.0))
            .collect();
        let shown = |p: f64| (!show.is_empty()).then(|| pct(&show, p));
        report.push(serde_json::json!({
            "card": name,
            "px": [r.width, r.height],
            "raster_p50_ms": pct(&raster, 0.5),
            "raster_p99_ms": pct(&raster, 0.99),
            "show_p50_ms": shown(0.5),
            "show_p99_ms": shown(0.99),
            "total_p50_ms": pct(&total, 0.5),
            "total_p99_ms": pct(&total, 0.99),
            "png": png,
        }));
    }
    #[cfg(windows)]
    let focus_kept = vutric_host_win::foreground_window() == focus_before;
    #[cfg(not(windows))]
    let focus_kept = true;

    // A real click on "Qabul qilish": it must reach the card and leave the foreground window where it was.
    #[cfg(windows)]
    let click = {
        let (_, deck) = &decks[0];
        let c = deck.card(card::qr).expect("a card");
        let r = card::render(&c, &fonts, &palette, scale);
        let at = vutric_host_win::card::placement(r.width, r.height);
        window
            .show(r.width, r.height, &r.bgra, at)
            .map_err(anyhow::Error::msg)?;
        vutric_host_win::card::pump_messages(100);
        let before = vutric_host_win::foreground_window();
        let target = r.buttons.iter().find(|(b, _)| *b == card::Button::Accept);
        let sent = target.is_some_and(|(_, b)| {
            vutric_host_win::card::synthetic_click(
                at.x + f32::midpoint(b[0], b[2]) as i32,
                at.y + f32::midpoint(b[1], b[3]) as i32,
            )
        });
        vutric_host_win::card::pump_messages(300);
        let hit = clicked.get().and_then(|(x, y)| r.hit(x, y));
        serde_json::json!({
            "sent": sent,
            "button": hit.map(|b| format!("{b:?}")),
            "focus_kept": vutric_host_win::foreground_window() == before,
        })
    };
    #[cfg(not(windows))]
    let click = serde_json::Value::Null;
    #[cfg(windows)]
    window.hide();

    let json = serde_json::json!({
        "runs": runs,
        "scale": scale,
        "fonts_load_ms": (fonts_ms * 1000.0).round() / 1000.0,
        "focus_kept": focus_kept,
        "click": click,
        "cards": report,
    });
    std::fs::write(&out, serde_json::to_string_pretty(&json)?)?;
    println!("{json}");
    Ok(())
}

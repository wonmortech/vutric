//! Card latency and focus on macOS (ADR-0118), the counterpart of W2's `crates/vutric-daemon/examples/cards.rs`:
//! the same four cards from W2's `Deck`, rendered by `vutric_overlay::card`, shown `--runs` times each in the
//! non-activating `CardPanel`, then one real click on "Qabul qilish". Writes the JSON (W2's fields plus the macOS
//! focus checks) and one PNG per card next to `--out`.
//!
//! ```text
//! cargo run -p vutric-host-mac --release --example cards -- --out cards-mac.json --runs 200 [--hold-ms 300]
//! ```
//!
//! Run it with another app frontmost (CI: `TextEdit`) so the focus check means something.

#[cfg(not(target_os = "macos"))]
fn main() {
    eprintln!("macOS only");
}

#[cfg(target_os = "macos")]
fn main() -> anyhow::Result<()> {
    mac::main()
}

#[cfg(target_os = "macos")]
mod mac {
    use std::cell::Cell;
    use std::path::PathBuf;
    use std::rc::Rc;
    use std::time::Instant;

    use anyhow::Context as _;
    use objc2::MainThreadMarker;
    use objc2_app_kit::{NSApplication, NSApplicationActivationPolicy};
    use vutric_daemon::cards::Deck;
    use vutric_daemon::i18n::Lang;
    use vutric_host_mac::OverlayLevel;
    use vutric_host_mac::card::{self as mac_card, CardPanel};
    use vutric_overlay::Palette;
    use vutric_overlay::card::{self, Button, Fonts};

    fn pct(v: &[f64], p: f64) -> f64 {
        let mut s = v.to_vec();
        s.sort_by(f64::total_cmp);
        s[((s.len() - 1) as f64 * p).round() as usize]
    }

    #[allow(clippy::too_many_lines, clippy::many_single_char_names)] // one measurement, read top to bottom; W2 names
    pub fn main() -> anyhow::Result<()> {
        let mut args = std::env::args().skip(1);
        let (mut out, mut runs, mut hold) = (PathBuf::from("cards-mac.json"), 200usize, 300u64);
        while let Some(a) = args.next() {
            match a.as_str() {
                "--out" => out = args.next().map_or(out, PathBuf::from),
                "--runs" => runs = args.next().and_then(|r| r.parse().ok()).unwrap_or(runs),
                // How long each card stays up after its runs (CI takes screenshots meanwhile).
                "--hold-ms" => hold = args.next().and_then(|r| r.parse().ok()).unwrap_or(hold),
                _ => {}
            }
        }
        let mtm = MainThreadMarker::new().context("not on the main thread")?;
        let app = NSApplication::sharedApplication(mtm);
        app.setActivationPolicy(NSApplicationActivationPolicy::Accessory);
        app.finishLaunching();

        let front_before = vutric_host_mac::frontmost_pid();
        let front_name = vutric_host_mac::frontmost_name();
        let me = std::process::id().cast_signed();

        let t0 = Instant::now();
        let fonts = Fonts::system();
        let fonts_ms = t0.elapsed().as_secs_f64() * 1000.0;
        let palette = Palette::vutric();
        let scale = mac_card::target_scale(mtm);

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
        d.pair("vutric://pair?v=1&id=k51tq3m0c2x8fz9e7d4a6b1h0g5j3n2p&addr=192.168.1.24:47820&code=73914");
        decks.push(("pair", d));

        let clicked = Rc::new(Cell::new(None::<(f32, f32)>));
        let c = Rc::clone(&clicked);
        let panel = CardPanel::new(mtm, OverlayLevel::Status, move |x, y| c.set(Some((x, y))));

        let (mut app_active, mut panel_key, mut front_changed, mut front_is_us) =
            (false, false, false, false);
        let mut check = |panel: &CardPanel| {
            app_active |= vutric_host_mac::app_is_active(mtm);
            panel_key |= panel.is_key();
            let f = vutric_host_mac::frontmost_pid();
            front_changed |= f != front_before;
            front_is_us |= f == me;
        };

        let mut report = Vec::new();
        let mut first_show_ms = None;
        for (name, deck) in &decks {
            let c = deck.card(card::qr).context("a card")?;
            let (mut raster, mut show) = (Vec::with_capacity(runs), Vec::with_capacity(runs));
            let mut last = None;
            for i in 0..runs.max(1) {
                let a = Instant::now();
                let r = card::render(&c, &fonts, &palette, scale as f32);
                raster.push(a.elapsed().as_secs_f64() * 1000.0);
                let b = Instant::now();
                let at = mac_card::placement(mtm, r.width, r.height, scale, 16.0);
                panel
                    .show(r.width, r.height, &r.bgra, at)
                    .map_err(anyhow::Error::msg)?;
                let ms = b.elapsed().as_secs_f64() * 1000.0;
                first_show_ms.get_or_insert(ms);
                show.push(ms);
                if i % 20 == 0 {
                    mac_card::pump(mtm, 1);
                    check(&panel);
                }
                last = Some(r);
            }
            mac_card::pump(mtm, hold);
            check(&panel);
            let r = last.context("at least one run")?;
            let mut rgba = r.bgra.clone();
            for p in rgba.chunks_exact_mut(4) {
                p.swap(0, 2);
            }
            let png = out.with_file_name(format!("card-mac-{name}.png"));
            image::RgbaImage::from_raw(r.width, r.height, rgba)
                .context("size matches")?
                .save(&png)?;
            let total: Vec<f64> = raster.iter().zip(&show).map(|(a, b)| a + b).collect();
            report.push(serde_json::json!({
                "card": name,
                "px": [r.width, r.height],
                "raster_p50_ms": pct(&raster, 0.5),
                "raster_p99_ms": pct(&raster, 0.99),
                "show_p50_ms": pct(&show, 0.5),
                "show_p99_ms": pct(&show, 0.99),
                "total_p50_ms": pct(&total, 0.5),
                "total_p99_ms": pct(&total, 0.99),
                "png": png,
            }));
        }

        // A real click on "Qabul qilish" through the window server.
        let (_, deck) = &decks[0];
        let c = deck.card(card::qr).context("a card")?;
        let r = card::render(&c, &fonts, &palette, scale as f32);
        let at = mac_card::placement(mtm, r.width, r.height, scale, 16.0);
        panel
            .show(r.width, r.height, &r.bgra, at)
            .map_err(anyhow::Error::msg)?;
        mac_card::pump(mtm, 300);
        let target = r.buttons.iter().find(|(b, _)| *b == Button::Accept);
        let sent = target.is_some_and(|(_, b)| {
            // Card pixels (top-left) to screen points (bottom-left).
            let x = at.origin.0 + f64::from(f32::midpoint(b[0], b[2])) / scale;
            let top = at.origin.1 + f64::from(r.height) / scale;
            let y = top - f64::from(f32::midpoint(b[1], b[3])) / scale;
            mac_card::synthetic_click(mtm, (x, y))
        });
        mac_card::pump(mtm, 500);
        check(&panel);
        let hit = clicked.get().and_then(|(x, y)| r.hit(x, y));
        let click = serde_json::json!({
            "sent": sent,
            "post_event_access": objc2_core_graphics::CGPreflightPostEventAccess(),
            "button": hit.map(|b| format!("{b:?}")),
            "frontmost_unchanged": vutric_host_mac::frontmost_pid() == front_before,
        });
        panel.hide();

        let json = serde_json::json!({
            "tool": "vutric-host-mac/examples/cards",
            "os": vutric_host_mac::os_version(),
            "arch": std::env::consts::ARCH,
            "runs": runs,
            "scale": scale,
            "fonts_load_ms": (fonts_ms * 1000.0).round() / 1000.0,
            "first_show_ms": first_show_ms,
            "frontmost_before": front_name,
            "focus_kept": !front_changed && !front_is_us && !app_active && !panel_key,
            "frontmost_changed": front_changed,
            "app_ever_active": app_active,
            "panel_ever_key": panel_key,
            "click": click,
            "cards": report,
        });
        std::fs::write(&out, serde_json::to_string_pretty(&json)?)?;
        println!("{json}");
        Ok(())
    }
}

/// A procedural "photo" (dusk over layered hills) so the spike runs without any asset.
pub fn dusk(width: u32, height: u32) -> image::RgbaImage {
    let sky_top = [18.0, 32.0, 74.0];
    let sky_mid = [64.0, 112.0, 168.0];
    let horizon = [236.0, 186.0, 112.0];
    let hills = [[34.0, 58.0, 84.0], [22.0, 40.0, 60.0], [12.0, 24.0, 38.0]];
    let (w, h) = (width as f32, height as f32);

    image::RgbaImage::from_fn(width, height, |x, y| {
        let (fx, fy) = (x as f32 / w, y as f32 / h);
        let mut c = if fy < 0.45 {
            mix(sky_top, sky_mid, fy / 0.45)
        } else {
            mix(sky_mid, horizon, ((fy - 0.45) / 0.25).min(1.0))
        };

        let sun = ((fx - 0.68).powi(2) + (fy - 0.56).powi(2) * 1.6).sqrt();
        let glow = (-sun * 9.0).exp();
        c = mix(c, [255.0, 226.0, 160.0], glow * 0.85);

        for (i, tone) in hills.iter().enumerate() {
            let k = i as f32;
            let ridge = 0.58
                + 0.09 * k
                + 0.035 * (fx * (6.0 + 3.0 * k) + k).sin()
                + 0.02 * (fx * 17.0 + k * 2.0).cos();
            if fy > ridge {
                c = *tone;
            }
        }
        image::Rgba([c[0] as u8, c[1] as u8, c[2] as u8, 255])
    })
}

fn mix(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    let t = t.clamp(0.0, 1.0);
    [
        a[0] + (b[0] - a[0]) * t,
        a[1] + (b[1] - a[1]) * t,
        a[2] + (b[2] - a[2]) * t,
    ]
}

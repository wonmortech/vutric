// Pufak: the card rides inside a soft refractive bubble that pops on landing. Slots (effects/pufak.rs):
//   fx[0] bubble x, y, radius px, opacity
//   fx[1] squash x, squash y, refraction px, pop progress (0 before, 1 done)
//   fx[2] time s, card shadow strength, unused, unused
// Cost inside the bubble rectangle: one texture sample (the refraction only moves its coordinate), a handful of
// exponentials, and 12 droplet discs during the 0.15 s pop.

@fragment
fn fs_main(@builtin(position) frag: vec4<f32>) -> @location(0) vec4<f32> {
    let px = frag.xy;
    let c = u.fx[0].xy;
    let r = max(u.fx[0].z, 1.0);
    let opacity = u.fx[0].w;
    let squash = max(u.fx[1].xy, vec2<f32>(0.5));
    let refraction = u.fx[1].z;
    let pop = u.fx[1].w;
    let time = u.fx[2].x;

    let q = (px - c) / (r * squash);
    let rr = length(q);
    let inside = 1.0 - smoothstep(0.98, 1.0, rr);
    let z = sqrt(max(1.0 - rr * rr, 0.0));

    // Lens: towards the rim the sample point is pulled to the centre, so the card looks magnified and curved.
    let lens = select(0.0, 1.0, opacity > 0.0) * inside;
    let warp = -q * (1.0 - z) * refraction * lens / max(u.card_xf.y, 0.001);
    var acc = draw_card(px, warp, u.fx[2].y);

    if (opacity > 0.0) {
        // A faint night-blue body so the bubble has volume over light wallpapers.
        let body_a = inside * 0.07 * opacity;
        acc = over(vec4<f32>(u.color_c.rgb * body_a, body_a), acc);

        // Thin-film rim: Fresnel-weighted, drifting between Signal and Glow, with a crisp outline.
        let fresnel = pow(1.0 - z, 3.0) * inside;
        let band = 0.5 + 0.5 * sin(atan2(q.y, q.x) * 2.0 + time * 1.3 + rr * 6.0);
        let film = mix(u.color_a.rgb, u.color_b.rgb, band);
        let outline = exp(-abs(rr - 1.0) * r / 1.4);
        let rim_a = clamp(fresnel * 0.55 + outline * 0.6, 0.0, 1.0) * opacity;
        let rim_rgb = mix(film, vec3<f32>(1.0), outline * 0.5);
        acc = over(vec4<f32>(rim_rgb * rim_a, rim_a), acc);

        // Two highlights: a soft window reflection top-left and a small one bottom-right.
        let h1 = q - vec2<f32>(-0.38, -0.42);
        let h2 = q - vec2<f32>(0.34, 0.40);
        let spec = (exp(-dot(h1, h1) * 40.0) * 0.6 + exp(-dot(h2, h2) * 120.0) * 0.3) * inside * opacity;
        acc = over(vec4<f32>(vec3<f32>(spec), spec), acc);
    }

    if (pop > 0.0 && pop < 1.0) {
        let left = 1.0 - pop;
        let spray = mix(u.color_b.rgb, vec3<f32>(1.0), 0.55);

        // The skin snaps into a ring that expands and fades.
        let ring_px = abs(length(px - c) / r - (1.0 + 0.30 * pop)) * r;
        let ring_a = left * left * exp(-ring_px / 2.0) * 0.8;
        acc = over(vec4<f32>(spray * ring_a, ring_a), acc);

        // Droplets thrown outwards, falling a little.
        for (var i = 0u; i < 12u; i = i + 1u) {
            let fi = f32(i);
            let a = fi * 0.5235988 + (hash11(fi + 5.0) - 0.5) * 0.4;
            let dist = (1.0 + 0.6 * pop * (0.7 + 0.6 * hash11(fi + 11.0))) * r;
            let p = c + vec2<f32>(cos(a), sin(a)) * dist + vec2<f32>(0.0, 40.0 * pop * pop);
            let dr = (3.5 * left + 1.0) * (0.7 + 0.6 * hash11(fi + 23.0));
            let da = (1.0 - smoothstep(dr - 0.75, dr + 0.75, length(px - p))) * left;
            acc = over(vec4<f32>(spray * da, da), acc);
        }
    }
    return acc;
}

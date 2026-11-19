// Kometa: the card is the head of a comet entering along the throw. A tapered streak and a stream of sparks trail
// it; a soft bloom marks the landing. Slots (effects/kometa.rs):
//   fx[0]    head x, y, head radius px, unused
//   fx[1..4] tail points, newest first: x, y, half width px, unused
//   fx[5]    tail alpha, spark spread px, spark flow phase, landing flash
//   fx[6]    travel direction x, y (unit), spark count, unused
// Cost inside the tail rectangle: 4 segment distances and at most fx[6].z sparks (each skipped unless within 4
// radii), so a full-HD frame stays far below the 2 ms budget.

// Width of the streak at polyline point k (0 = head).
fn width_at(k: u32) -> f32 {
    return select(u.fx[k].z, u.fx[0].z * 0.6, k == 0u);
}

fn streak(px: vec2<f32>) -> vec4<f32> {
    let alpha = u.fx[5].x;
    if (alpha <= 0.0) {
        return vec4<f32>(0.0);
    }
    var best = 1.0e9;
    var along = 1.0;
    for (var k = 0u; k < 4u; k = k + 1u) {
        let seg = segment(px, u.fx[k].xy, u.fx[k + 1u].xy);
        let w = max(mix(width_at(k), width_at(k + 1u), seg.y), 0.5);
        let d = seg.x / w;
        if (d < best) {
            best = d;
            along = (f32(k) + seg.y) / 4.0;
        }
    }
    let core = exp(-best * best * 2.0);
    let glow = exp(-best * 1.2) * 0.35;
    let fade = pow(1.0 - along, 1.4);
    let tint = mix(u.color_b.rgb, u.color_a.rgb, along);
    let rgb = mix(tint, vec3<f32>(1.0), core * (1.0 - along) * 0.6);
    let a = alpha * fade * clamp(core * 0.75 + glow, 0.0, 1.0);
    return vec4<f32>(rgb * a, a);
}

fn sparks(px: vec2<f32>) -> vec4<f32> {
    let alpha = u.fx[5].x;
    if (alpha <= 0.0) {
        return vec4<f32>(0.0);
    }
    let spread = u.fx[5].y;
    let flow = u.fx[5].z;
    let n = u32(u.fx[6].z);
    let dir = u.fx[6].xy;
    let side_dir = vec2<f32>(-dir.y, dir.x);
    var acc = vec4<f32>(0.0);
    for (var i = 0u; i < n; i = i + 1u) {
        let fi = f32(i);
        let h1 = hash11(fi);
        let h2 = hash11(fi + 31.0);
        let h3 = hash11(fi + 67.0);
        // Where along the tail this spark is now: born at the head, drifting back and dying at the end.
        let s = fract(h1 + flow * (0.55 + 0.45 * h2));
        let seg_f = s * 4.0;
        let k = min(u32(seg_f), 3u);
        let base = mix(u.fx[k].xy, u.fx[k + 1u].xy, seg_f - f32(k));
        let pos = base + side_dir * ((h3 - 0.5) * 2.0 * spread * (0.25 + s));
        let r = mix(3.2, 1.0, s) * (0.7 + 0.6 * h2);
        let d = length(px - pos);
        if (d < r * 4.0) {
            let a = alpha * (1.0 - s) * exp(-(d * d) / (r * r));
            let rgb = mix(vec3<f32>(1.0), u.color_b.rgb, s);
            acc = over(vec4<f32>(rgb * a, a), acc);
        }
    }
    return acc;
}

fn bloom(px: vec2<f32>) -> vec4<f32> {
    let f = u.fx[5].w;
    if (f <= 0.0) {
        return vec4<f32>(0.0);
    }
    let r = max(u.fx[0].z, 1.0);
    let d = max(length(px - u.fx[0].xy) - r, 0.0);
    let a = f * 0.55 * exp(-d / (r * 0.55));
    let rgb = mix(u.color_b.rgb, vec3<f32>(1.0), 0.35);
    return vec4<f32>(rgb * a, a);
}

@fragment
fn fs_main(@builtin(position) frag: vec4<f32>) -> @location(0) vec4<f32> {
    let px = frag.xy;
    var acc = streak(px);
    acc = over(sparks(px), acc);
    acc = over(bloom(px), acc);
    return over(draw_card(px, vec2<f32>(0.0), 0.36), acc);
}

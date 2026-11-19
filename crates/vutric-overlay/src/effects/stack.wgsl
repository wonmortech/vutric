// Many files: blank cards fanned behind the landed top card and a counter badge on its corner. Drawn as an overlay
// with premultiplied blending, once per pixel over the union of its regions. Slots (effects/stack.rs):
//   fx[0] files, cards behind (0..=3), appear (0..1, overshoots), opacity
//   fx[1] badge x, y, radius px, unused
//   fx[2] offset per card behind x, y px, rotation per card behind rad, unused
// The uniform card pose is the top card's. Digits are seven-segment distance fields, so no font or texture.

fn seg_mask(d: u32) -> u32 {
    // Bits: a top, b top right, c bottom right, d bottom, e bottom left, f top left, g middle.
    var masks = array<u32, 10>(0x3Fu, 0x06u, 0x5Bu, 0x4Fu, 0x66u, 0x6Du, 0x7Du, 0x07u, 0x7Fu, 0x6Fu);
    return masks[min(d, 9u)];
}

// Coverage of one digit centred at the origin, in badge units (badge radius = 1, y down); `aa` is one pixel.
fn digit(p: vec2<f32>, d: u32, aa: f32) -> f32 {
    let m = seg_mask(d);
    let hw = 0.22;
    let hh = 0.42;
    let thick = 0.075;
    let tl = vec2<f32>(-hw, -hh);
    let tr = vec2<f32>(hw, -hh);
    let ml = vec2<f32>(-hw, 0.0);
    let mr = vec2<f32>(hw, 0.0);
    let bl = vec2<f32>(-hw, hh);
    let br = vec2<f32>(hw, hh);
    var best = 1.0e9;
    if ((m & 1u) != 0u) { best = min(best, segment(p, tl, tr).x); }
    if ((m & 2u) != 0u) { best = min(best, segment(p, tr, mr).x); }
    if ((m & 4u) != 0u) { best = min(best, segment(p, mr, br).x); }
    if ((m & 8u) != 0u) { best = min(best, segment(p, bl, br).x); }
    if ((m & 16u) != 0u) { best = min(best, segment(p, ml, bl).x); }
    if ((m & 32u) != 0u) { best = min(best, segment(p, tl, ml).x); }
    if ((m & 64u) != 0u) { best = min(best, segment(p, ml, mr).x); }
    return 1.0 - smoothstep(thick - aa, thick + aa, best);
}

@fragment
fn fs_main(@builtin(position) frag: vec4<f32>) -> @location(0) vec4<f32> {
    let px = frag.xy;
    let files = u32(u.fx[0].x);
    let behind = u32(u.fx[0].y);
    let appear = u.fx[0].z;
    let opacity = u.fx[0].w;
    if (opacity <= 0.0) {
        return vec4<f32>(0.0);
    }
    let scale = max(u.card_xf.y, 0.001);
    let half_size = u.card.zw;
    let corner = u.card_xf.w;

    // The main layer drew the top card; hide the stack where it covers.
    let top_d = sd_round_box(card_local(px), half_size, corner) * scale;
    let top_a = (1.0 - smoothstep(-0.75, 0.75, top_d)) * u.card_xf.z;

    var acc = vec4<f32>(0.0);
    for (var j = 0u; j < behind; j = j + 1u) {
        // Farthest first.
        let k = f32(behind - j);
        let center = u.card.xy + u.fx[2].xy * k * appear;
        let turn = u.card_xf.x + u.fx[2].z * k * appear;
        let local = rotate(px - center, -turn) / scale;
        let ds = sd_round_box(local - vec2<f32>(0.0, 14.0) / scale, half_size, corner) * scale;
        let shadow_a = 0.20 * (1.0 - smoothstep(-20.0, 30.0, ds));
        acc = over(vec4<f32>(0.0, 0.0, 0.0, shadow_a), acc);
        let d = sd_round_box(local, half_size, corner) * scale;
        let card_a = 1.0 - smoothstep(-0.75, 0.75, d);
        let face = mix(u.color_c.rgb, vec3<f32>(1.0), 0.16 + 0.05 * (3.0 - k));
        let rim = (1.0 - smoothstep(0.0, 1.6, abs(d + 1.2))) * 0.30;
        let rgb = mix(face, u.color_b.rgb, rim);
        acc = over(vec4<f32>(rgb * card_a, card_a), acc);
    }
    acc = acc * ((1.0 - top_a) * opacity);

    let bc = u.fx[1].xy;
    let radius = u.fx[1].z * max(appear, 0.0);
    if (radius > 1.0) {
        let bd = length(px - bc) - radius;
        let badge_a = (1.0 - smoothstep(-0.75, 0.75, bd)) * opacity;
        let ring = (1.0 - smoothstep(0.0, 1.5, abs(bd + 1.5))) * 0.9;
        let fill = mix(u.color_a.rgb, vec3<f32>(1.0), ring);
        acc = over(vec4<f32>(fill * badge_a, badge_a), acc);

        let p = (px - bc) / radius;
        let aa = 1.0 / radius;
        let n = min(files, 99u);
        var cover = 0.0;
        if (n >= 10u) {
            cover = max(digit(p - vec2<f32>(-0.27, 0.0), n / 10u, aa), digit(p - vec2<f32>(0.27, 0.0), n % 10u, aa));
        } else {
            cover = digit(p, n, aa);
        }
        let digit_a = cover * badge_a;
        acc = over(vec4<f32>(vec3<f32>(digit_a), digit_a), acc);
    }
    return acc;
}

// Halqa: a luminous portal opens at the screen edge, the card flies through it with a light trail and lands
// with a soft shadow. Everything is analytic (signed distance fields) in one full-screen pass; the renderer
// scissors it to the effect bounds. Output is premultiplied alpha in linear space.

struct Uniforms {
    screen: vec4<f32>,      // width, height, time, unused
    ring: vec4<f32>,        // center x, center y, radius, opacity
    ring_style: vec4<f32>,  // aspect, thickness px, phase, unused
    card: vec4<f32>,        // center x, center y, half width, half height
    card_xf: vec4<f32>,     // rotation, scale, opacity, corner radius px
    trail: array<vec4<f32>, 4>, // x, y, alpha, scale
    color_a: vec4<f32>,
    color_b: vec4<f32>,
    color_c: vec4<f32>,
};

@group(0) @binding(0) var<uniform> u: Uniforms;
@group(0) @binding(1) var card_tex: texture_2d<f32>;
@group(0) @binding(2) var card_sampler: sampler;

@vertex
fn vs_main(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
    let x = f32(i32(i & 1u) * 4 - 1);
    let y = f32(i32(i >> 1u) * 4 - 1);
    return vec4<f32>(x, y, 0.0, 1.0);
}

fn sd_round_box(p: vec2<f32>, half_size: vec2<f32>, r: f32) -> f32 {
    let q = abs(p) - half_size + vec2<f32>(r);
    return length(max(q, vec2<f32>(0.0))) + min(max(q.x, q.y), 0.0) - r;
}

fn over(front: vec4<f32>, back: vec4<f32>) -> vec4<f32> {
    return front + back * (1.0 - front.a);
}

fn rotate(p: vec2<f32>, a: f32) -> vec2<f32> {
    let c = cos(a);
    let s = sin(a);
    return vec2<f32>(c * p.x - s * p.y, s * p.x + c * p.y);
}

fn portal(px: vec2<f32>) -> vec4<f32> {
    let r = u.ring.z;
    let opacity = u.ring.w;
    if (opacity <= 0.0 || r <= 0.5) {
        return vec4<f32>(0.0);
    }
    let rp = px - u.ring.xy;
    let q = vec2<f32>(rp.x / u.ring_style.x, rp.y);
    let dist = length(q);
    let ang = atan2(q.y, q.x);
    let phase = u.ring_style.z;

    // The hole: a dark lens with slow concentric ripples, so the portal reads as depth, not a sticker.
    let inside = 1.0 - smoothstep(r * 0.80, r, dist);
    let ripple = 0.5 + 0.5 * sin(dist * 0.09 - phase * 2.6);
    let hole_a = inside * (0.30 + 0.10 * ripple) * opacity;
    var acc = vec4<f32>(u.color_c.rgb * hole_a, hole_a);

    // The rim: a thin hot core, a coloured band that drifts around it, and a wide soft glow.
    let thickness = u.ring_style.y;
    let d = abs(dist - r) - thickness * 0.5;
    let core = 1.0 - smoothstep(-1.0, 1.0, d);
    let glow = exp(-max(d, 0.0) / (r * 0.10)) * 0.65;
    let band = 0.5 + 0.5 * sin(ang * 1.5 + phase);
    let tint = mix(u.color_a.rgb, u.color_b.rgb, band);
    let rim_rgb = mix(tint, vec3<f32>(1.0), core * 0.55);
    let rim_a = clamp(core + glow * (1.0 - core), 0.0, 1.0) * opacity;
    acc = over(vec4<f32>(rim_rgb * rim_a, rim_a), acc);
    return acc;
}

@fragment
fn fs_main(@builtin(position) frag: vec4<f32>) -> @location(0) vec4<f32> {
    let px = frag.xy;
    var acc = portal(px);

    let half_size = u.card.zw;
    let corner = u.card_xf.w;
    let scale = max(u.card_xf.y, 0.001);
    let opacity = u.card_xf.z;

    for (var i = 0u; i < 4u; i = i + 1u) {
        let g = u.trail[i];
        if (g.z > 0.0) {
            let gs = max(g.w, 0.001);
            // Echoes of the card's lit edge, not filled ghosts: a bright contour with a faint interior.
            let d = sd_round_box((px - g.xy) / gs, half_size, corner) * gs;
            let edge = exp(-abs(d) / 9.0);
            let interior = select(0.0, 0.10, d < 0.0);
            let a = g.z * clamp(edge + interior, 0.0, 1.0);
            let col = mix(u.color_b.rgb, u.color_a.rgb, f32(i) / 3.0);
            acc = over(vec4<f32>(col * a, a), acc);
        }
    }

    let local = rotate(px - u.card.xy, -u.card_xf.x) / scale;

    // Sampled unconditionally: texture sampling must stay in uniform control flow.
    let uv = local / (2.0 * half_size) + vec2<f32>(0.5);
    let tex = textureSample(card_tex, card_sampler, clamp(uv, vec2<f32>(0.0), vec2<f32>(1.0)));

    if (opacity > 0.0) {
        let shadow_offset = vec2<f32>(0.0, 22.0) / scale;
        let ds = sd_round_box(local - shadow_offset, half_size, corner) * scale;
        let shadow_a = 0.42 * (1.0 - smoothstep(-30.0, 46.0, ds)) * opacity;
        acc = over(vec4<f32>(0.0, 0.0, 0.0, shadow_a), acc);

        let d_px = sd_round_box(local, half_size, corner) * scale;
        let card_a = (1.0 - smoothstep(-0.75, 0.75, d_px)) * opacity;
        let rim = (1.0 - smoothstep(0.0, 1.6, abs(d_px + 1.2))) * 0.38;
        let sheen = smoothstep(0.55, 0.0, uv.y) * 0.06;
        let rgb = mix(tex.rgb, vec3<f32>(1.0), clamp(rim + sheen, 0.0, 1.0));
        acc = over(vec4<f32>(rgb * card_a, card_a), acc);
    }

    return acc;
}

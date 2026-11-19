// Shared prelude of the effects next to Halqa (crates/vutric-overlay/src/effects). Each effect's WGSL file is
// appended to this text and adds `fs_main`. One uniform layout for all of them, mirrored by `scene::Uniforms`.
// Output is premultiplied alpha in linear space, like halqa.wgsl.

struct Scene {
    screen: vec4<f32>,          // width, height, time, unused
    card: vec4<f32>,            // center x, center y, half width, half height
    card_xf: vec4<f32>,         // rotation, scale, opacity, corner radius px
    fx: array<vec4<f32>, 8>,    // effect parameters; each effect documents its slots
    color_a: vec4<f32>,         // Signal 500, linear
    color_b: vec4<f32>,         // Glow 200, linear
    color_c: vec4<f32>,         // Night 950, linear
};

@group(0) @binding(0) var<uniform> u: Scene;
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

// Cheap stable pseudo-random value in [0, 1) for a small integer seed.
fn hash11(n: f32) -> f32 {
    return fract(sin(n * 12.9898 + 4.1414) * 43758.5453);
}

// Distance from p to the segment a-b, and where along it (0 at a, 1 at b) the closest point lies.
fn segment(p: vec2<f32>, a: vec2<f32>, b: vec2<f32>) -> vec2<f32> {
    let pa = p - a;
    let ba = b - a;
    let h = clamp(dot(pa, ba) / max(dot(ba, ba), 0.0001), 0.0, 1.0);
    return vec2<f32>(length(pa - ba * h), h);
}

// The card's local position (unrotated, at scale 1) for a screen pixel.
fn card_local(px: vec2<f32>) -> vec2<f32> {
    let scale = max(u.card_xf.y, 0.001);
    return rotate(px - u.card.xy, -u.card_xf.x) / scale;
}

// The flung card at the uniform pose: a soft drop shadow of strength `shadow`, then the image with a lit rim and a
// top sheen. `warp` moves the texture sample point in card-local pixels (refraction); zero for a plain card.
// textureSampleLevel needs no uniform control flow, so this may be called from anywhere.
fn draw_card(px: vec2<f32>, warp: vec2<f32>, shadow: f32) -> vec4<f32> {
    let opacity = u.card_xf.z;
    if (opacity <= 0.0) {
        return vec4<f32>(0.0);
    }
    let half_size = u.card.zw;
    let corner = u.card_xf.w;
    let scale = max(u.card_xf.y, 0.001);
    let local = card_local(px);

    var acc = vec4<f32>(0.0);
    if (shadow > 0.0) {
        let shadow_offset = vec2<f32>(0.0, 22.0) / scale;
        let ds = sd_round_box(local - shadow_offset, half_size, corner) * scale;
        let shadow_a = shadow * (1.0 - smoothstep(-30.0, 46.0, ds)) * opacity;
        acc = vec4<f32>(0.0, 0.0, 0.0, shadow_a);
    }

    let uv = (local + warp) / (2.0 * half_size) + vec2<f32>(0.5);
    let tex = textureSampleLevel(card_tex, card_sampler, clamp(uv, vec2<f32>(0.0), vec2<f32>(1.0)), 0.0);
    let d_px = sd_round_box(local, half_size, corner) * scale;
    let card_a = (1.0 - smoothstep(-0.75, 0.75, d_px)) * opacity;
    let rim = (1.0 - smoothstep(0.0, 1.6, abs(d_px + 1.2))) * 0.38;
    let sheen = smoothstep(0.55, 0.0, uv.y) * 0.06;
    let rgb = mix(tex.rgb, vec3<f32>(1.0), clamp(rim + sheen, 0.0, 1.0));
    return over(vec4<f32>(rgb * card_a, card_a), acc);
}

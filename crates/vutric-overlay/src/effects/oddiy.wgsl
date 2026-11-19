// Oddiy: the card alone with a soft shadow. All of its motion lives in the timeline (effects/oddiy.rs); no fx
// slots are used. The cheapest effect: one texture sample and two rounded-box distances per pixel.

@fragment
fn fs_main(@builtin(position) frag: vec4<f32>) -> @location(0) vec4<f32> {
    return draw_card(frag.xy, vec2<f32>(0.0), 0.30);
}

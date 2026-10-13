@group(1) @binding(0)
var sky_texture: texture_cube<f32>;
@group(1) @binding(1)
var sky_sampler: sampler;

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) ndc: vec2<f32>,
};

/// One triangle that covers the screen.
@vertex
fn vs_main(@builtin(vertex_index) index: u32) -> VertexOutput {
    let ndc = vec2<f32>(f32((index << 1u) & 2u), f32(index & 2u)) * 2.0 - 1.0;
    var out: VertexOutput;
    out.clip_position = vec4<f32>(ndc, 1.0, 1.0);
    out.ndc = ndc;
    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> SceneOutput {
    let far = frame.inverse_view_projection * vec4<f32>(in.ndc, 1.0, 1.0);
    let direction = far.xyz / far.w - frame.camera_position;
    // Cubemaps are sampled left-handed and the world is right-handed, so flip z to keep the image
    // the right way round.
    let day = textureSample(sky_texture, sky_sampler, direction * vec3<f32>(1.0, 1.0, -1.0)).rgb;
    // The image is daytime only: it is tinted at dusk and fades to the horizon colour at night.
    var out: SceneOutput;
    out.color = vec4<f32>(mix(frame.fog_color, day * frame.sky_tint, frame.daylight), 1.0);
    out.emissive = vec4<f32>(0.0, 0.0, 0.0, 1.0);
    return out;
}

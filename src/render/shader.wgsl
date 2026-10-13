@group(1) @binding(0)
var block_texture: texture_2d_array<f32>;
@group(1) @binding(1)
var block_sampler: sampler;

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) @interpolate(flat) normal: vec3<f32>,
    @location(2) occlusion: f32,
    @location(3) @interpolate(flat) tile: u32,
};

@vertex
fn vs_main(
    @location(0) position: vec3<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) normal: vec3<f32>,
    @location(3) occlusion: f32,
    @location(4) tile: u32,
) -> VertexOutput {
    var out: VertexOutput;
    out.clip_position = frame.view_projection * vec4<f32>(position, 1.0);
    out.uv = uv;
    out.normal = normal;
    out.occlusion = occlusion;
    out.tile = tile;
    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> SceneOutput {
    let color = textureSample(block_texture, block_sampler, in.uv, in.tile);
    var out: SceneOutput;
    out.color = vec4<f32>(color.rgb * light(in.normal) * in.occlusion, color.a);
    out.emissive = vec4<f32>(0.0, 0.0, 0.0, 1.0);
    return out;
}

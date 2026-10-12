@group(0) @binding(0)
var<uniform> transform: mat4x4<f32>;

@group(1) @binding(0)
var block_texture: texture_2d<f32>;
@group(1) @binding(1)
var block_sampler: sampler;

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) shade: f32,
};

@vertex
fn vs_main(@location(0) position: vec3<f32>, @location(1) uv: vec2<f32>, @location(2) shade: f32) -> VertexOutput {
    var out: VertexOutput;
    out.clip_position = transform * vec4<f32>(position, 1.0);
    out.uv = uv;
    out.shade = shade;
    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let color = textureSample(block_texture, block_sampler, in.uv);
    return vec4<f32>(color.rgb * in.shade, color.a);
}

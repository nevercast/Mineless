/// Six body part transforms per avatar instance, in the order of `PARTS`.
@group(1) @binding(0)
var<storage, read> part_transforms: array<mat4x4<f32>>;
@group(1) @binding(1)
var skin: texture_2d<f32>;
@group(1) @binding(2)
var skin_sampler: sampler;

const PART_COUNT: u32 = 6u;

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) @interpolate(flat) normal: vec3<f32>,
};

@vertex
fn vs_main(
    @builtin(instance_index) instance: u32,
    @location(0) position: vec3<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) normal: vec3<f32>,
    @location(3) part: u32,
) -> VertexOutput {
    var out: VertexOutput;
    // Part transforms are rigid, so they turn normals the same way they turn positions.
    let transform = part_transforms[instance * PART_COUNT + part];
    out.clip_position = frame.view_projection * transform * vec4<f32>(position, 1.0);
    out.uv = uv;
    out.normal = (transform * vec4<f32>(normal, 0.0)).xyz;
    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let color = textureSample(skin, skin_sampler, in.uv);
    return vec4<f32>(color.rgb * light(in.normal), color.a);
}

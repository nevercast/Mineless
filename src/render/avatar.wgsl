struct Instance {
    /// Body part transforms in the order of `PARTS`.
    parts: array<mat4x4<f32>, 6>,
    /// Layer of `skins` this avatar wears.
    skin: u32,
};

@group(1) @binding(0)
var<storage, read> instances: array<Instance>;
@group(1) @binding(1)
var skins: texture_2d_array<f32>;
@group(1) @binding(2)
var skin_sampler: sampler;

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) @interpolate(flat) normal: vec3<f32>,
    @location(2) @interpolate(flat) skin: u32,
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
    let transform = instances[instance].parts[part];
    out.clip_position = frame.view_projection * transform * vec4<f32>(position, 1.0);
    out.uv = uv;
    out.normal = (transform * vec4<f32>(normal, 0.0)).xyz;
    out.skin = instances[instance].skin;
    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let color = textureSample(skins, skin_sampler, in.uv, in.skin);
    // The base layer is opaque; empty parts of the overlay layer are cut out.
    if color.a < 0.5 {
        discard;
    }
    return vec4<f32>(color.rgb * light(in.normal), 1.0);
}

struct Instance {
    /// Body part transforms in the order of `PARTS`.
    parts: array<mat4x4<f32>, 6>,
    /// Layer of the skin arrays this avatar wears.
    skin: u32,
    /// Offsets this avatar's glow animation from others wearing the same skin.
    seed: f32,
};

@group(1) @binding(0)
var<storage, read> instances: array<Instance>;

@group(2) @binding(0)
var skin_albedo: texture_2d_array<f32>;
@group(2) @binding(1)
var skin_emissive: texture_2d_array<f32>;
@group(2) @binding(2)
var skin_spill: texture_2d_array<f32>;
@group(2) @binding(3)
var skin_sampler: sampler;

/// Skin texels per block, matching `TEXELS_PER_BLOCK` in avatar.rs.
const TEXELS_PER_BLOCK: f32 = 16.0 / 0.9375;

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) @interpolate(flat) normal: vec3<f32>,
    @location(2) @interpolate(flat) skin: u32,
    @location(3) @interpolate(flat) seed: f32,
    /// Rest-pose position in skin texels, so the glow pattern sticks to the body as it moves.
    @location(4) model_position: vec3<f32>,
    @location(5) world_position: vec3<f32>,
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
    let world_position = transform * vec4<f32>(position, 1.0);
    out.clip_position = frame.view_projection * world_position;
    out.world_position = world_position.xyz;
    out.uv = uv;
    out.normal = (transform * vec4<f32>(normal, 0.0)).xyz;
    out.skin = instances[instance].skin;
    out.seed = instances[instance].seed;
    out.model_position = position * TEXELS_PER_BLOCK;
    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> SceneOutput {
    let albedo = textureSample(skin_albedo, skin_sampler, in.uv, in.skin);
    // The base layer is opaque; empty parts of the overlay layer are cut out.
    if albedo.a < 0.5 {
        discard;
    }
    let emissive = textureSample(skin_emissive, skin_sampler, in.uv, in.skin).r;
    let spill = textureSample(skin_spill, skin_sampler, in.uv, in.skin).rgb;
    let m = skin_glow_modulation(in.model_position, frame.time, in.seed);
    let glow = albedo.rgb * emissive * m * SKIN_EMISSIVE_STRENGTH;
    var out: SceneOutput;
    out.color = vec4<f32>(albedo.rgb * (light(in.normal) + spill * m) + glow, 1.0);
    out.emissive = vec4<f32>(glow, 1.0);
    return fogged(out, in.world_position);
}

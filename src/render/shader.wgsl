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
    @location(4) world_position: vec3<f32>,
};

/// Unpacks `Vertex` from mesher.rs. Normals are in the order of `FACES`, and the corner picks
/// its texture coordinates the same way up on every face.
@vertex
fn vs_main(@location(0) packed: u32, @location(1) chunk_origin: vec3<f32>) -> VertexOutput {
    var normals = array<vec3<f32>, 6>(
        vec3<f32>(1.0, 0.0, 0.0), vec3<f32>(-1.0, 0.0, 0.0),
        vec3<f32>(0.0, 1.0, 0.0), vec3<f32>(0.0, -1.0, 0.0),
        vec3<f32>(0.0, 0.0, 1.0), vec3<f32>(0.0, 0.0, -1.0),
    );
    var uvs = array<vec2<f32>, 4>(
        vec2<f32>(0.0, 1.0), vec2<f32>(1.0, 1.0), vec2<f32>(1.0, 0.0), vec2<f32>(0.0, 0.0),
    );
    // Brightness for a corner touched by 0, 1, 2 or 3 solid blocks.
    var occlusion = array<f32, 4>(1.0, 0.8, 0.65, 0.5);

    let local = vec3<f32>(vec3<u32>(packed, packed >> 5u, packed >> 10u) & vec3<u32>(31u));
    let position = chunk_origin + local;
    var out: VertexOutput;
    out.clip_position = frame.view_projection * vec4<f32>(position, 1.0);
    out.uv = uvs[(packed >> 18u) & 3u];
    out.normal = normals[(packed >> 15u) & 7u];
    out.occlusion = occlusion[(packed >> 20u) & 3u];
    out.tile = packed >> 22u;
    out.world_position = position;
    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> SceneOutput {
    let color = textureSample(block_texture, block_sampler, in.uv, in.tile);
    var out: SceneOutput;
    out.color = vec4<f32>(color.rgb * light(in.normal) * in.occlusion, color.a);
    out.emissive = vec4<f32>(0.0, 0.0, 0.0, 1.0);
    return fogged(out, in.world_position);
}

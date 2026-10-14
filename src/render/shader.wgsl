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
    return shade(in);
}

/// A face lit, textured and fogged. Entry points can't call each other, so every one that draws
/// faces calls this.
fn shade(in: VertexOutput) -> SceneOutput {
    let color = textureSample(block_texture, block_sampler, in.uv, in.tile);
    var out: SceneOutput;
    out.color = vec4<f32>(color.rgb * light(in.normal) * in.occlusion, color.a);
    out.emissive = vec4<f32>(0.0, 0.0, 0.0, 1.0);
    return fogged(out, in.world_position);
}

/// Water nearer than the cutaway's plane is left out with the terrain.
@fragment
fn fs_water(in: VertexOutput) -> SceneOutput {
    let out = shade(in);
    if dot(frame.cut_plane.xyz, in.world_position) < frame.cut_plane.w {
        discard;
    }
    return out;
}

/// Terrain for a camera inside rock. Everything nearer than the cut plane is left out. The terrain
/// mesh is closed, so where the plane passes through rock, the first face the view ray meets past
/// the plane is the back of the one where it leaves that rock, and that face is drawn as the cut
/// stone on the plane in front of it. It keeps its own depth: moved onto the plane, the back of a
/// far face would cover a near face seen through open space.
@fragment
fn fs_cutaway(in: VertexOutput, @builtin(front_facing) front_facing: bool) -> SceneOutput {
    let plane = frame.cut_plane;
    let camera = frame.camera_position;
    let ray = in.world_position - camera;
    let on_plane = camera + ray * ((plane.w - dot(plane.xyz, camera)) / dot(plane.xyz, ray));
    // Stone faces the plane from whichever axis is closest to the view, one texture per block.
    let facing = abs(plane.xyz);
    var uv = on_plane.xz;
    if facing.x >= facing.y && facing.x >= facing.z {
        uv = on_plane.zy;
    } else if facing.z >= facing.y {
        uv = on_plane.xy;
    }
    // Sampled at the top level of detail, since `fract` jumps at block edges and would confuse the
    // choice of level there.
    let stone = textureSampleLevel(block_texture, block_sampler, fract(uv), STONE_TILE, 0.0).rgb;
    var out = shade(in);
    if !front_facing {
        // Darker than any lit face, so the cut stands apart from the tunnels seen through it.
        var cut: SceneOutput;
        cut.color = vec4<f32>(stone * 0.35, 1.0);
        cut.emissive = vec4<f32>(0.0, 0.0, 0.0, 1.0);
        out = fogged(cut, on_plane);
    }
    if dot(plane.xyz, in.world_position) < plane.w {
        discard;
    }
    return out;
}

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) world_position: vec3<f32>,
};

@vertex
fn vs_main(@location(0) position: vec3<f32>) -> VertexOutput {
    var out: VertexOutput;
    out.clip_position = frame.view_projection * vec4<f32>(position, 1.0);
    out.world_position = position;
    return out;
}

/// Blocks across one hexagon of the shield pattern, and its lines' width as a fraction of that.
const CELL: f32 = 4.0;
const LINE: f32 = 0.03;
const SHIELD_COLOR: vec3<f32> = vec3<f32>(0.35, 0.8, 1.0);
/// Line opacity from afar, and close up so the barrier announces itself as you reach it.
const FAR_ALPHA: f32 = 0.1;
const NEAR_ALPHA: f32 = 0.25;
const NEAR: f32 = 8.0;
const FAR: f32 = 16.0;
/// Cell interiors are just short of invisible.
const FILL_ALPHA: f32 = 0.01;

/// Distance from `p` to the nearest edge of a honeycomb whose hexagons are one unit across their
/// flats.
fn hex_edge_distance(p: vec2<f32>) -> f32 {
    let r = vec2<f32>(1.0, 1.7320508);
    let h = r * 0.5;
    let a = p - r * floor(p / r) - h;
    let b = p - h - r * floor((p - h) / r) - h;
    let centre_offset = select(b, a, dot(a, a) < dot(b, b));
    let q = abs(centre_offset);
    return 0.5 - max(dot(q, normalize(r)), q.x);
}

@fragment
fn fs_main(in: VertexOutput) -> SceneOutput {
    // Each wall lies in a plane of constant x or z, so their sum runs along it.
    let along = in.world_position.x + in.world_position.z;
    let edge = hex_edge_distance(vec2<f32>(along, in.world_position.y) / CELL);
    let line = 1.0 - smoothstep(LINE, LINE + fwidth(edge), edge);
    let near = 1.0 - smoothstep(NEAR, FAR, distance(in.world_position, frame.camera_position));
    let alpha = max(line * mix(FAR_ALPHA, NEAR_ALPHA, near), FILL_ALPHA);
    var out: SceneOutput;
    out.color = vec4<f32>(SHIELD_COLOR, alpha);
    out.emissive = vec4<f32>(0.0, 0.0, 0.0, 1.0);
    return fogged(out, in.world_position);
}

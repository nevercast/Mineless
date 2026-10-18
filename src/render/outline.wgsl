/// Half the ribbon's width per block of distance from the camera, so it stays the same size on screen: about four pixels
/// either side of the edge in a 1080 pixel tall window.
const HALF_WIDTH: f32 = 0.005;
/// Ribbons are pulled this fraction of the way to the camera so the faces meeting at their edge don't cut into them.
const PULL: f32 = 0.01;
const OPACITY: f32 = 0.8;

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) world_position: vec3<f32>,
    /// -1 on one long side of the ribbon and 1 on the other.
    @location(1) side: f32,
};

@vertex
fn vs_main(@location(0) start: vec3<f32>, @location(1) end: vec3<f32>, @location(2) corner: vec2<f32>) -> VertexOutput {
    let along = normalize(end - start);
    var position = mix(start, end, corner.x);
    let half_width = HALF_WIDTH * distance(position, frame.camera_position);
    // Running the ribbon past the corners fills the gap where three ribbons meet.
    position += along * (corner.x * 2.0 - 1.0) * half_width;
    let across = cross(along, frame.camera_position - position);
    position += across * inverseSqrt(max(dot(across, across), 1e-12)) * corner.y * half_width;
    position = mix(position, frame.camera_position, PULL);

    var out: VertexOutput;
    out.clip_position = frame.view_projection * vec4<f32>(position, 1.0);
    out.world_position = position;
    out.side = corner.y;
    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> SceneOutput {
    // Pixels from the ribbon's long edge, which fades over the last pixel so the line is smooth.
    let coverage = clamp((1.0 - abs(in.side)) / fwidth(in.side), 0.0, 1.0);
    var out: SceneOutput;
    out.color = vec4<f32>(0.0, 0.0, 0.0, OPACITY * coverage);
    out.emissive = vec4<f32>(0.0, 0.0, 0.0, 1.0);
    return fogged(out, in.world_position);
}

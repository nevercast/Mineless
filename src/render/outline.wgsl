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

@fragment
fn fs_main(in: VertexOutput) -> SceneOutput {
    var out: SceneOutput;
    out.color = vec4<f32>(0.0, 0.0, 0.0, 0.5);
    out.emissive = vec4<f32>(0.0, 0.0, 0.0, 1.0);
    return fogged(out, in.world_position);
}

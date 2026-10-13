// Per-frame uniforms shared by every pipeline at group 0. Prepended to each shader's source.

struct Frame {
    view_projection: mat4x4<f32>,
    /// Unit vector toward the sun or moon.
    light_direction: vec3<f32>,
    /// Seconds since midnight. A day is a whole number of skin glow loops, so it wraps cleanly.
    time: f32,
    light_color: vec3<f32>,
    ambient: vec3<f32>,
    camera_position: vec3<f32>,
    /// Matches the sky behind the world, so distant terrain fades into it.
    fog_color: vec3<f32>,
};

/// Fog starts this many blocks from the camera and hides everything past `FOG_END`.
const FOG_START: f32 = 64.0;
const FOG_END: f32 = 192.0;

@group(0) @binding(0)
var<uniform> frame: Frame;

/// Every scene pipeline writes lit colour and, for bloom, the light a surface gives off itself.
struct SceneOutput {
    @location(0) color: vec4<f32>,
    @location(1) emissive: vec4<f32>,
};

/// How much fog covers a fragment, from 0 near the camera to 1 at `FOG_END`.
fn fog(world_position: vec3<f32>) -> f32 {
    return smoothstep(FOG_START, FOG_END, distance(world_position, frame.camera_position));
}

/// Fogs the scene output. Glow fades with distance too, so far avatars don't bloom through fog.
fn fogged(out: SceneOutput, world_position: vec3<f32>) -> SceneOutput {
    let f = fog(world_position);
    var fogged_out = out;
    fogged_out.color = vec4<f32>(mix(out.color.rgb, frame.fog_color, f), out.color.a);
    fogged_out.emissive = vec4<f32>(out.emissive.rgb * (1.0 - f), out.emissive.a);
    return fogged_out;
}

/// Light reaching a surface facing `normal`: the sky's ambient plus the sun or moon.
fn light(normal: vec3<f32>) -> vec3<f32> {
    return frame.ambient + frame.light_color * max(dot(normal, frame.light_direction), 0.0);
}

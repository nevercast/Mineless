// Per-frame uniforms shared by every pipeline at group 0. Prepended to each shader's source.

struct Frame {
    view_projection: mat4x4<f32>,
    /// Unit vector toward the sun or moon.
    light_direction: vec3<f32>,
    /// Seconds since midnight. A day is a whole number of skin glow loops, so it wraps cleanly.
    time: f32,
    light_color: vec3<f32>,
    ambient: vec3<f32>,
};

@group(0) @binding(0)
var<uniform> frame: Frame;

/// Every scene pipeline writes lit colour and, for bloom, the light a surface gives off itself.
struct SceneOutput {
    @location(0) color: vec4<f32>,
    @location(1) emissive: vec4<f32>,
};

/// Light reaching a surface facing `normal`: the sky's ambient plus the sun or moon.
fn light(normal: vec3<f32>) -> vec3<f32> {
    return frame.ambient + frame.light_color * max(dot(normal, frame.light_direction), 0.0);
}

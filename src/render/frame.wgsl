// Per-frame uniforms shared by every pipeline at group 0. Prepended to each shader's source.

struct Frame {
    view_projection: mat4x4<f32>,
    /// Unit vector toward the sun or moon.
    light_direction: vec3<f32>,
    light_color: vec3<f32>,
    ambient: vec3<f32>,
};

@group(0) @binding(0)
var<uniform> frame: Frame;

/// Light reaching a surface facing `normal`: the sky's ambient plus the sun or moon.
fn light(normal: vec3<f32>) -> vec3<f32> {
    return frame.ambient + frame.light_color * max(dot(normal, frame.light_direction), 0.0);
}

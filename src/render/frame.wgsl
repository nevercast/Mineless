// Per-frame uniforms shared by every pipeline at group 0. Prepended to each shader's source.

struct Frame {
    view_projection: mat4x4<f32>,
    inverse_view_projection: mat4x4<f32>,
    /// Unit vector toward the sun or moon.
    light_direction: vec3<f32>,
    /// Seconds since midnight. A day is a whole number of skin glow loops, so it wraps cleanly.
    time: f32,
    light_color: vec3<f32>,
    ambient: vec3<f32>,
    camera_position: vec3<f32>,
    /// Matches the sky behind the world, so distant terrain fades into it.
    fog_color: vec3<f32>,
    /// What the daytime skybox image is multiplied by, warm at dusk.
    sky_tint: vec3<f32>,
    /// 1 by day and 0 at night: how much of the skybox shows over the plain horizon colour.
    daylight: f32,
};

/// Air within this many blocks of a camera at sea level is clear, and the clear stretch grows
/// by `FOG_START_PER_BLOCK` for every block the camera is above the sea, so high views see far.
const FOG_START: f32 = 128.0;
const FOG_START_PER_BLOCK: f32 = 2.0;
/// Haze per block of view at sea level, thinning by a factor of e every `FOG_FALLOFF` blocks
/// higher. Lowlands fade out by about 350 blocks; peaks stay crisp from the valleys.
const FOG_DENSITY: f32 = 0.018;
const FOG_FALLOFF: f32 = 20.0;

@group(0) @binding(0)
var<uniform> frame: Frame;

/// Every scene pipeline writes lit colour and, for bloom, the light a surface gives off itself.
struct SceneOutput {
    @location(0) color: vec4<f32>,
    @location(1) emissive: vec4<f32>,
};

/// How much fog covers a fragment, from 0 near the camera towards 1. Haze is gathered along the
/// view ray past the clear stretch, at the ray's mean density between its two ends' heights.
fn fog(world_position: vec3<f32>) -> f32 {
    let camera = frame.camera_position;
    let clear = FOG_START + FOG_START_PER_BLOCK * max(camera.y - SEA_LEVEL, 0.0);
    let hazy = max(distance(world_position, camera) - clear, 0.0);
    let a = max(camera.y - SEA_LEVEL, 0.0) / FOG_FALLOFF;
    let b = max(world_position.y - SEA_LEVEL, 0.0) / FOG_FALLOFF;
    var density = exp(-a);
    if abs(b - a) > 1e-3 {
        density = (exp(-a) - exp(-b)) / (b - a);
    }
    return 1.0 - exp(-FOG_DENSITY * hazy * density);
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

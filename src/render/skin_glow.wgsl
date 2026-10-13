// Glow, spill and their animated fade and throb for the character skins, ported from the skin
// handover's skin_glow.glsl.

// 3D gradient noise, roughly -1..1. It repeats every SKIN_NOISE_PERIOD cells, which is what lets
// skin_glow_modulation wrap time seamlessly.
const SKIN_NOISE_PERIOD: f32 = 64.0;

fn skin_hash33(cell: vec3<f32>) -> vec3<f32> {
    // Periodic lattice; also keeps the sin() inputs small.
    let c = cell - SKIN_NOISE_PERIOD * floor(cell / SKIN_NOISE_PERIOD);
    let h = vec3<f32>(
        dot(c, vec3<f32>(127.1, 311.7, 74.7)),
        dot(c, vec3<f32>(269.5, 183.3, 246.1)),
        dot(c, vec3<f32>(113.5, 271.9, 124.6)),
    );
    return -1.0 + 2.0 * fract(sin(h) * 43758.5453123);
}

fn skin_corner(i: vec3<f32>, f: vec3<f32>, corner: vec3<f32>) -> f32 {
    return dot(skin_hash33(i + corner), f - corner);
}

fn skin_noise3(p: vec3<f32>) -> f32 {
    let i = floor(p);
    let f = fract(p);
    let u = f * f * f * (f * (f * 6.0 - 15.0) + 10.0);
    let n000 = skin_corner(i, f, vec3<f32>(0.0, 0.0, 0.0));
    let n100 = skin_corner(i, f, vec3<f32>(1.0, 0.0, 0.0));
    let n010 = skin_corner(i, f, vec3<f32>(0.0, 1.0, 0.0));
    let n110 = skin_corner(i, f, vec3<f32>(1.0, 1.0, 0.0));
    let n001 = skin_corner(i, f, vec3<f32>(0.0, 0.0, 1.0));
    let n101 = skin_corner(i, f, vec3<f32>(1.0, 0.0, 1.0));
    let n011 = skin_corner(i, f, vec3<f32>(0.0, 1.0, 1.0));
    let n111 = skin_corner(i, f, vec3<f32>(1.0, 1.0, 1.0));
    return mix(
        mix(mix(n000, n100, u.x), mix(n010, n110, u.x), u.y),
        mix(mix(n001, n101, u.x), mix(n011, n111, u.x), u.y),
        u.z,
    );
}

// 1 / feature size in model pixels (about 6 pixel patches).
const SKIN_GLOW_SCALE: f32 = 0.16;
// Lights stay at 25% or more of the current pulse.
const SKIN_GLOW_FLOOR: f32 = 0.25;
// Whole-body pulse depth and period.
const SKIN_GLOW_THROB_DEPTH: f32 = 0.35;
const SKIN_GLOW_THROB_SECS: f32 = 1.0;
// Glow brightness on the HDR target. The handover suggests 1.5 to 3.
const SKIN_EMISSIVE_STRENGTH: f32 = 2.0;
// Drift in noise units per second for the two octaves. Each component times SKIN_GLOW_LOOP_SECS
// is a whole multiple of SKIN_NOISE_PERIOD, so the pattern returns to its start and time can wrap
// without a visible jump.
const SKIN_GLOW_FLOW1: vec3<f32> = vec3<f32>(0.0, -2.56, 0.0);
const SKIN_GLOW_FLOW2: vec3<f32> = vec3<f32>(3.68, 0.0, -4.80);
const SKIN_GLOW_LOOP_SECS: f32 = 400.0;

/// Brightness multiplier, 0..1, for the glow and its spill at a rest-pose model position.
fn skin_glow_modulation(model_position: vec3<f32>, time: f32, seed: f32) -> f32 {
    let t = time - SKIN_GLOW_LOOP_SECS * floor(time / SKIN_GLOW_LOOP_SECS);
    let p = model_position + vec3<f32>(seed);
    var n = 0.7 * skin_noise3(p * SKIN_GLOW_SCALE + SKIN_GLOW_FLOW1 * t)
        + 0.3 * skin_noise3(p * (SKIN_GLOW_SCALE * 2.3) + SKIN_GLOW_FLOW2 * t + 17.0);
    n = 0.5 + 0.5 * n / 0.75;
    let throb = 1.0 - SKIN_GLOW_THROB_DEPTH
        * (0.5 - 0.5 * cos(6.2831853 * t / SKIN_GLOW_THROB_SECS));
    return throb * mix(SKIN_GLOW_FLOOR, 1.0, smoothstep(0.2, 0.8, n));
}

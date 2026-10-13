// Bloom from the emissive target, then tonemapping of the HDR scene onto the swapchain.

@group(0) @binding(0)
var source: texture_2d<f32>;
@group(0) @binding(1)
var source_sampler: sampler;
@group(0) @binding(2)
var bloom: texture_2d<f32>;

/// How much of the blurred glow lands on the final image.
const BLOOM_STRENGTH: f32 = 0.6;

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

/// One triangle that covers the screen, so no vertex buffer is needed.
@vertex
fn vs_fullscreen(@builtin(vertex_index) index: u32) -> VertexOutput {
    let uv = vec2<f32>(f32((index << 1u) & 2u), f32(index & 2u));
    var out: VertexOutput;
    out.clip_position = vec4<f32>(uv * vec2<f32>(2.0, -2.0) + vec2<f32>(-1.0, 1.0), 0.0, 1.0);
    out.uv = uv;
    return out;
}

fn tap(uv: vec2<f32>, texel: vec2<f32>, x: f32, y: f32) -> vec3<f32> {
    return textureSample(source, source_sampler, uv + texel * vec2<f32>(x, y)).rgb;
}

/// Halves the resolution with the 13-tap filter from Call of Duty: Advanced Warfare, which keeps
/// small bright texels from flickering as they move across the coarser grid.
@fragment
fn fs_downsample(in: VertexOutput) -> @location(0) vec4<f32> {
    let texel = 1.0 / vec2<f32>(textureDimensions(source));
    let center = tap(in.uv, texel, -1.0, -1.0) + tap(in.uv, texel, 1.0, -1.0)
        + tap(in.uv, texel, -1.0, 1.0) + tap(in.uv, texel, 1.0, 1.0);
    let corners = tap(in.uv, texel, -2.0, -2.0) + tap(in.uv, texel, 2.0, -2.0)
        + tap(in.uv, texel, -2.0, 2.0) + tap(in.uv, texel, 2.0, 2.0);
    let edges = tap(in.uv, texel, 0.0, -2.0) + tap(in.uv, texel, -2.0, 0.0)
        + tap(in.uv, texel, 2.0, 0.0) + tap(in.uv, texel, 0.0, 2.0);
    let middle = tap(in.uv, texel, 0.0, 0.0);
    let color = center * 0.125 + corners * 0.03125 + edges * 0.0625 + middle * 0.125;
    return vec4<f32>(color, 1.0);
}

/// Blurs a coarser level with a 3x3 tent and adds it onto the next finer one.
@fragment
fn fs_upsample(in: VertexOutput) -> @location(0) vec4<f32> {
    let texel = 1.0 / vec2<f32>(textureDimensions(source));
    let corners = tap(in.uv, texel, -1.0, -1.0) + tap(in.uv, texel, 1.0, -1.0)
        + tap(in.uv, texel, -1.0, 1.0) + tap(in.uv, texel, 1.0, 1.0);
    let edges = tap(in.uv, texel, 0.0, -1.0) + tap(in.uv, texel, -1.0, 0.0)
        + tap(in.uv, texel, 1.0, 0.0) + tap(in.uv, texel, 0.0, 1.0);
    let color = (corners + edges * 2.0 + tap(in.uv, texel, 0.0, 0.0) * 4.0) / 16.0;
    return vec4<f32>(color, 1.0);
}

/// Khronos PBR Neutral: leaves colours below about 0.8 untouched, so a lit day scene looks as it
/// did before HDR, and only rolls off highlights such as glow.
fn tonemap(color: vec3<f32>) -> vec3<f32> {
    let start = 0.8 - 0.04;
    let desaturation = 0.15;
    let x = min(color.r, min(color.g, color.b));
    let offset = select(0.04, x - 6.25 * x * x, x < 0.08);
    var c = color - offset;
    let peak = max(c.r, max(c.g, c.b));
    if peak < start {
        return c;
    }
    let d = 1.0 - start;
    let new_peak = 1.0 - d * d / (peak + d - start);
    c *= new_peak / peak;
    let g = 1.0 - 1.0 / (desaturation * (peak - new_peak) + 1.0);
    return mix(c, vec3<f32>(new_peak), g);
}

@fragment
fn fs_composite(in: VertexOutput) -> @location(0) vec4<f32> {
    let scene = textureSample(source, source_sampler, in.uv).rgb;
    let glow = textureSample(bloom, source_sampler, in.uv).rgb;
    return vec4<f32>(tonemap(scene + glow * BLOOM_STRENGTH), 1.0);
}

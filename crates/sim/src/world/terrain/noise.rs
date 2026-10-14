//! Seeded gradient noise and the small maths around it, in plain arithmetic so every platform
//! generates the same world.

use glam::{IVec3, Vec2, Vec3};

/// No gradient noise sample, or fractal sum of them, is further than this from 0.
pub(super) const NOISE_BOUND: f32 = 1.1;

/// `octaves` layers of noise, each twice as fine and half as strong as the last, scaled back to
/// the range of one layer. `layer` gets the layer's frequency and a salt that makes the layers
/// independent.
pub(super) fn fractal(octaves: u32, layer: impl Fn(f32, u32) -> f32) -> f32 {
    let mut total = 0.0;
    let mut strength = 1.0;
    let mut strengths = 0.0;
    let mut frequency = 1.0;
    for octave in 0..octaves {
        total += strength * layer(frequency, octave.wrapping_mul(0x68e3_1da4));
        strengths += strength;
        strength *= 0.5;
        frequency *= 2.0;
    }
    total / strengths
}

/// Gradient noise in about `-1..=1`: a hashed gradient at each lattice corner, dotted with the
/// offset to the point and blended with a smooth fade. Plain arithmetic only, so every platform
/// generates the same world.
pub(super) fn gradient_2d(seed: u32, point: Vec2) -> f32 {
    let (x, z) = (floor(point.x), floor(point.y));
    let t = point - Vec2::new(x as f32, z as f32);
    let corner = |dx: i32, dz: i32| {
        let offset = t - Vec2::new(dx as f32, dz as f32);
        let (gx, gz) = match hash(seed, x + dx, 0, z + dz) & 7 {
            0 => (1.0, 1.0),
            1 => (-1.0, 1.0),
            2 => (1.0, -1.0),
            3 => (-1.0, -1.0),
            4 => (1.0, 0.0),
            5 => (-1.0, 0.0),
            6 => (0.0, 1.0),
            _ => (0.0, -1.0),
        };
        gx * offset.x + gz * offset.y
    };
    let (fx, fz) = (fade(t.x), fade(t.y));
    lerp(
        lerp(corner(0, 0), corner(1, 0), fx),
        lerp(corner(0, 1), corner(1, 1), fx),
        fz,
    )
}

/// 3D gradient noise in about `-1..=1`, with gradients towards the twelve edges of a cube.
pub(super) fn gradient_3d(seed: u32, point: Vec3) -> f32 {
    let base = IVec3::new(floor(point.x), floor(point.y), floor(point.z));
    let t = point - base.as_vec3();
    let corner = |d: IVec3| {
        let o = t - d.as_vec3();
        let p = base + d;
        match hash(seed, p.x, p.y, p.z) % 12 {
            0 => o.x + o.y,
            1 => -o.x + o.y,
            2 => o.x - o.y,
            3 => -o.x - o.y,
            4 => o.x + o.z,
            5 => -o.x + o.z,
            6 => o.x - o.z,
            7 => -o.x - o.z,
            8 => o.y + o.z,
            9 => -o.y + o.z,
            10 => o.y - o.z,
            _ => -o.y - o.z,
        }
    };
    let f = Vec3::new(fade(t.x), fade(t.y), fade(t.z));
    let plane = |z: i32| {
        lerp(
            lerp(
                corner(IVec3::new(0, 0, z)),
                corner(IVec3::new(1, 0, z)),
                f.x,
            ),
            lerp(
                corner(IVec3::new(0, 1, z)),
                corner(IVec3::new(1, 1, z)),
                f.x,
            ),
            f.y,
        )
    };
    lerp(plane(0), plane(1), f.z)
}

/// An integer hash of a lattice point, so every platform agrees.
pub(super) fn hash(seed: u32, x: i32, y: i32, z: i32) -> u32 {
    let mut hash = seed
        ^ (x as u32).wrapping_mul(0x27d4_eb2d)
        ^ (y as u32).wrapping_mul(0x9e37_79b1).rotate_left(8)
        ^ (z as u32).wrapping_mul(0x1656_67b1).rotate_left(16);
    hash ^= hash >> 15;
    hash = hash.wrapping_mul(0x2c1b_3c6d);
    hash ^= hash >> 12;
    hash = hash.wrapping_mul(0x297a_2d39);
    hash ^= hash >> 15;
    hash
}

/// The largest whole number no greater than `x`, for any `x` in `i32` range. `f32::floor`
/// becomes a library call on baseline x86-64, which has no rounding instruction, and noise floors
/// several coordinates per sample.
fn floor(x: f32) -> i32 {
    let truncated = x as i32;
    if (truncated as f32) > x {
        truncated - 1
    } else {
        truncated
    }
}

/// Eases lattice blending so the noise has no visible creases at cell edges.
fn fade(t: f32) -> f32 {
    t * t * t * (t * (t * 6.0 - 15.0) + 10.0)
}

pub(super) fn lerp(from: f32, to: f32, t: f32) -> f32 {
    from + (to - from) * t
}

pub(super) fn smoothstep(edge0: f32, edge1: f32, x: f32) -> f32 {
    let t = ((x - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn floor_matches_the_library_floor() {
        for x in [
            -2.5,
            -2.0,
            -1.0001,
            -0.5,
            -0.0,
            0.0,
            0.25,
            1.0,
            1.9999,
            1e6 + 0.5,
            -1e6 - 0.5,
        ] {
            assert_eq!(floor(x), x.floor() as i32, "{x}");
        }
    }

    #[test]
    fn noise_stays_near_unit_range() {
        for x in -40..40 {
            for z in -40..40 {
                let point = Vec2::new(x as f32, z as f32) * 0.37;
                assert!(gradient_2d(3, point).abs() <= NOISE_BOUND);
                assert!(gradient_3d(3, point.extend(z as f32 * 0.21)).abs() <= NOISE_BOUND);
            }
        }
    }
}

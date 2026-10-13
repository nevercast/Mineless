//! Time of day and the light the sky gives at that time.

use std::f32::consts::TAU;

use glam::{Vec3, vec3};

/// Ticks in one day: twenty minutes at 20 ticks a second, as in Minecraft.
pub const DAY_TICKS: u64 = 24_000;

/// How far the sun's path leans from passing straight overhead, so faces toward it and away
/// from it read differently even at noon.
const SUN_TILT: f32 = 0.45;

// Linear colours.
const SUN: Vec3 = vec3(1.0, 0.95, 0.85);
const LOW_SUN: Vec3 = vec3(1.0, 0.55, 0.3);
const MOON: Vec3 = vec3(0.12, 0.14, 0.22);
const DAY_AMBIENT: Vec3 = vec3(0.42, 0.45, 0.5);
const NIGHT_AMBIENT: Vec3 = vec3(0.08, 0.09, 0.15);
const DAY_SKY: Vec3 = vec3(0.53, 0.72, 0.92);
const DUSK_SKY: Vec3 = vec3(0.85, 0.45, 0.25);
const NIGHT_SKY: Vec3 = vec3(0.01, 0.012, 0.03);

/// The light falling on the world at one moment, in linear colour.
pub struct Sky {
    /// Unit vector toward the light: the sun by day and the moon, opposite it, by night.
    pub light_direction: Vec3,
    pub light_color: Vec3,
    /// Light from the whole sky, reaching every face whichever way it points.
    pub ambient: Vec3,
    /// The colour behind everything.
    pub color: Vec3,
}

impl Sky {
    /// `day_fraction` is how far through the day it is, from midnight at 0: sunrise is at 0.25,
    /// noon at 0.5 and sunset at 0.75. The sun rises in +X and sets in -X.
    pub fn at(day_fraction: f32) -> Self {
        let (height, across) = ((day_fraction - 0.25) * TAU).sin_cos();
        let sun = vec3(across, height * SUN_TILT.cos(), height * SUN_TILT.sin());
        let daylight = smoothstep(-0.15, 0.25, sun.y);
        // Whichever of the sun and moon is up fades out at the horizon, so swapping them is
        // seamless.
        let (light_direction, light_color) = if sun.y >= 0.0 {
            let color = LOW_SUN.lerp(SUN, smoothstep(0.0, 0.4, sun.y));
            (sun, color * smoothstep(0.0, 0.1, sun.y))
        } else {
            (-sun, MOON * smoothstep(0.0, 0.1, -sun.y))
        };
        let dusk = 1.0 - smoothstep(0.0, 0.3, sun.y.abs());
        Self {
            light_direction,
            light_color,
            ambient: NIGHT_AMBIENT.lerp(DAY_AMBIENT, daylight),
            color: NIGHT_SKY.lerp(DAY_SKY, daylight).lerp(DUSK_SKY, dusk * 0.5),
        }
    }
}

fn smoothstep(edge0: f32, edge1: f32, x: f32) -> f32 {
    let t = ((x - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sun_is_high_at_noon_and_the_moon_at_midnight() {
        let noon = Sky::at(0.5);
        assert!(noon.light_direction.y > 0.8);
        let midnight = Sky::at(0.0);
        assert!(midnight.light_direction.y > 0.8);
        assert!(midnight.light_color.max_element() < noon.light_color.min_element());
        assert!(midnight.ambient.max_element() < noon.ambient.min_element());
    }

    #[test]
    fn light_fades_out_at_the_horizon() {
        for horizon in [0.25, 0.75] {
            let sky = Sky::at(horizon);
            assert!(sky.light_color.max_element() < 1e-3, "{horizon}");
            assert!(sky.light_direction.is_normalized());
        }
    }
}

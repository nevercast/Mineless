use std::f32::consts::FRAC_PI_2;

use glam::Vec3;
use winit::event::{ElementState, KeyEvent};
use winit::keyboard::{KeyCode, PhysicalKey};

use crate::render::Camera;

const SPEED: f32 = 12.0;
const MOUSE_SENSITIVITY: f32 = 0.0025;
/// Stop just short of straight up or down so the view never flips.
const MAX_PITCH: f32 = FRAC_PI_2 - 0.01;

/// Free-flying controls for a human watching the world: WASD moves relative to where the camera
/// faces, Space and Shift move straight up and down, and mouse motion turns the view.
#[derive(Default)]
pub struct Spectator {
    forward: bool,
    back: bool,
    left: bool,
    right: bool,
    up: bool,
    down: bool,
}

impl Spectator {
    pub fn key(&mut self, event: &KeyEvent) {
        let PhysicalKey::Code(code) = event.physical_key else {
            return;
        };
        let held = event.state == ElementState::Pressed;
        match code {
            KeyCode::KeyW => self.forward = held,
            KeyCode::KeyS => self.back = held,
            KeyCode::KeyA => self.left = held,
            KeyCode::KeyD => self.right = held,
            KeyCode::Space => self.up = held,
            KeyCode::ShiftLeft => self.down = held,
            _ => {}
        }
    }

    pub fn release_all(&mut self) {
        *self = Self::default();
    }

    pub fn look(&self, camera: &mut Camera, dx: f64, dy: f64) {
        camera.yaw -= dx as f32 * MOUSE_SENSITIVITY;
        camera.pitch = (camera.pitch - dy as f32 * MOUSE_SENSITIVITY).clamp(-MAX_PITCH, MAX_PITCH);
    }

    pub fn update(&self, camera: &mut Camera, seconds: f32) {
        // Walk along the ground plane regardless of pitch, like a Minecraft spectator.
        let forward = Vec3::new(-camera.yaw.sin(), 0.0, -camera.yaw.cos());
        let right = forward.cross(Vec3::Y);
        let axis = |positive: bool, negative: bool| i32::from(positive) as f32 - i32::from(negative) as f32;
        let direction = forward * axis(self.forward, self.back) + right * axis(self.right, self.left) + Vec3::Y * axis(self.up, self.down);
        camera.position += direction.normalize_or_zero() * SPEED * seconds;
    }
}

use std::f32::consts::FRAC_PI_2;

use glam::Vec3;
use winit::event::{ElementState, KeyEvent};
use winit::keyboard::{KeyCode, PhysicalKey};

use crate::render::{Camera, ListLine};

const SPEED: f32 = 12.0;
const MOUSE_SENSITIVITY: f32 = 0.0025;
/// Stop just short of straight up or down so the view never flips.
const MAX_PITCH: f32 = FRAC_PI_2 - 0.01;
/// How far back from an agent's eyes the camera sits when it jumps to the agent or follows it, unless something is in the way,
/// and how far below level a jump leaves it looking.
pub const FOLLOW_DISTANCE: f32 = 6.0;
const JUMP_PITCH: f32 = -0.35;
const LIST_KEY: KeyCode = KeyCode::Tab;
const JUMP_KEY: KeyCode = KeyCode::KeyT;
const FOLLOW_KEY: KeyCode = KeyCode::KeyF;

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

    /// Whether a movement key is held.
    pub fn is_moving(&self) -> bool {
        self.forward || self.back || self.left || self.right || self.up || self.down
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

/// The spectator's list of agents: Tab shows it, the arrow keys pick one, T jumps the camera to it, and F follows it. Following
/// starts behind the agent and keeps the camera on it, and turning the view orbits it.
#[derive(Default)]
pub struct AgentList {
    pub open: bool,
    selected: usize,
    following: Option<usize>,
}

impl AgentList {
    /// Acts on a key pressed, when there are `agents` agents. Returns the agent to jump the camera behind, for the jump key or
    /// on starting to follow.
    pub fn key(&mut self, code: KeyCode, agents: usize) -> Option<usize> {
        if code == LIST_KEY {
            self.open = !self.open;
        }
        if agents == 0 {
            return None;
        }
        match code {
            KeyCode::ArrowDown => self.selected = (self.selected + 1) % agents,
            KeyCode::ArrowUp => self.selected = (self.selected + agents - 1) % agents,
            JUMP_KEY => {
                self.following = None;
                return Some(self.selected);
            }
            FOLLOW_KEY => {
                self.following = (self.following != Some(self.selected)).then_some(self.selected);
                return self.following;
            }
            _ => {}
        }
        None
    }

    pub fn following(&self) -> Option<usize> {
        self.following
    }

    pub fn stop_following(&mut self) {
        self.following = None;
    }

    /// The lines to show for agents with these names and distances from the camera, or none while the list is closed.
    pub fn lines(&self, agents: &[(&str, f32)]) -> Vec<ListLine> {
        if !self.open {
            return Vec::new();
        }
        let line = |text: String| ListLine { text, selected: false };
        if agents.is_empty() {
            return vec![line("No agents: add some to agents.toml".to_owned())];
        }
        let width = agents.iter().map(|(name, _)| name.chars().count()).max().unwrap_or(0);
        let mut lines = vec![line("Agents  Up/Down pick  T go to  F follow".to_owned())];
        lines.extend(agents.iter().enumerate().map(|(index, (name, distance))| {
            let following = if self.following == Some(index) { "  following" } else { "" };
            ListLine {
                text: format!("{name:<width$} {distance:>5.0} m{following}"),
                selected: index == self.selected,
            }
        }));
        lines
    }
}

/// Turns the camera to face the way `yaw` does, looking a little down, ready to `orbit` from behind.
pub fn face(camera: &mut Camera, yaw: f32) {
    camera.yaw = yaw;
    camera.pitch = JUMP_PITCH;
}

/// Puts the camera `distance` back from `focus` along its view, so turning the view orbits `focus`.
pub fn orbit(camera: &mut Camera, focus: Vec3, distance: f32) {
    camera.position = focus - camera.forward() * distance;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_arrows_pick_an_agent_round_the_list() {
        let mut list = AgentList::default();
        list.key(KeyCode::ArrowUp, 3);
        assert_eq!(list.selected, 2);
        list.key(KeyCode::ArrowDown, 3);
        assert_eq!(list.selected, 0);
        assert_eq!(list.key(KeyCode::ArrowDown, 3), None);
        assert_eq!(list.key(JUMP_KEY, 3), Some(1));
    }

    #[test]
    fn follow_toggles_and_jumping_stops_it() {
        let mut list = AgentList::default();
        assert_eq!(list.key(FOLLOW_KEY, 2), Some(0));
        assert_eq!(list.following(), Some(0));
        list.key(KeyCode::ArrowDown, 2);
        assert_eq!(list.key(FOLLOW_KEY, 2), Some(1));
        assert_eq!(list.following(), Some(1));
        assert_eq!(list.key(FOLLOW_KEY, 2), None);
        assert_eq!(list.following(), None);
        list.key(FOLLOW_KEY, 2);
        list.key(JUMP_KEY, 2);
        assert_eq!(list.following(), None);
    }

    #[test]
    fn with_no_agents_only_tab_does_anything() {
        let mut list = AgentList::default();
        for code in [KeyCode::ArrowDown, JUMP_KEY, FOLLOW_KEY] {
            assert_eq!(list.key(code, 0), None);
        }
        assert_eq!(list.following(), None);
        assert!(list.lines(&[]).is_empty());
        list.key(LIST_KEY, 0);
        assert_eq!(list.lines(&[]).len(), 1);
    }

    #[test]
    fn lines_line_up_names_and_distances_and_mark_the_picked_and_followed() {
        let mut list = AgentList::default();
        list.key(LIST_KEY, 2);
        list.key(KeyCode::ArrowDown, 2);
        list.key(FOLLOW_KEY, 2);
        let lines = list.lines(&[("Wanda", 12.4), ("Al", 103.6)]);
        let texts: Vec<&str> = lines.iter().map(|line| line.text.as_str()).collect();
        assert_eq!(texts[1..], ["Wanda    12 m", "Al      104 m  following"]);
        let selected: Vec<bool> = lines.iter().map(|line| line.selected).collect();
        assert_eq!(selected, [false, false, true]);
    }

    #[test]
    fn orbiting_keeps_the_focus_dead_ahead_at_the_follow_distance() {
        let mut camera = Camera::looking_at(Vec3::ZERO, Vec3::NEG_Z);
        let focus = Vec3::new(10.0, 70.0, -4.0);
        for (yaw, pitch) in [(0.0, 0.0), (1.0, -0.5), (-2.5, 0.8)] {
            camera.yaw = yaw;
            camera.pitch = pitch;
            orbit(&mut camera, focus, FOLLOW_DISTANCE);
            assert!((camera.position.distance(focus) - FOLLOW_DISTANCE).abs() < 1e-4);
            assert!((focus - camera.position).normalize().dot(camera.forward()) > 0.9999);
        }
    }
}

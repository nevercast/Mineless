mod avatar;
mod mesher;
mod render;
mod sky;
mod spectator;

use std::f32::consts::{PI, TAU};
use std::sync::Arc;
use std::time::Instant;

use winit::application::ApplicationHandler;
use winit::event::{DeviceEvent, DeviceId, ElementState, MouseButton, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::window::{CursorGrabMode, Window, WindowId};

use glam::{IVec3, Vec2, Vec3, vec3};
use render::{Camera, Renderer};
use sim::avatar::{Avatar, Skin};
use sim::player::{JUMP_HEIGHTS, Player, TICK_RATE};
use sim::world::{self, World};
use sky::{DAY_TICKS, Sky};
use spectator::Spectator;

struct App {
    world: World,
    players: Vec<Player>,
    /// Simulation ticks since the world began.
    ticks: u64,
    /// Seconds of simulation time owed but not yet ticked.
    unsimulated: f32,
    window: Option<Arc<Window>>,
    renderer: Option<Renderer>,
    camera: Camera,
    spectator: Spectator,
    mouse_captured: bool,
    last_frame: Instant,
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let attributes = Window::default_attributes().with_title("Mineless");
        let window = Arc::new(event_loop.create_window(attributes).expect("create window"));
        let mut renderer = pollster::block_on(Renderer::new(window.clone()));
        for chunk in self.world.chunk_positions() {
            renderer.upload_chunk_mesh(chunk, &mesher::build_chunk_mesh(&self.world, chunk));
        }
        self.renderer = Some(renderer);
        self.window = Some(window);
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        let (Some(window), Some(renderer)) = (&self.window, &mut self.renderer) else {
            return;
        };
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => renderer.resize(size.width, size.height),
            WindowEvent::KeyboardInput { event, .. } => {
                if event.physical_key == PhysicalKey::Code(KeyCode::Escape) {
                    self.mouse_captured = set_mouse_capture(window, false);
                }
                self.spectator.key(&event);
            }
            WindowEvent::MouseInput {
                state: ElementState::Pressed,
                button: MouseButton::Left,
                ..
            } => self.mouse_captured = set_mouse_capture(window, true),
            WindowEvent::Focused(false) => {
                self.spectator.release_all();
                self.mouse_captured = set_mouse_capture(window, false);
            }
            WindowEvent::RedrawRequested => {
                let now = Instant::now();
                let seconds = (now - self.last_frame).as_secs_f32();
                self.last_frame = now;
                self.spectator.update(&mut self.camera, seconds);
                // Simulate in fixed ticks; a long stall drops time rather than ticking to catch up.
                self.unsimulated = (self.unsimulated + seconds).min(MAX_CATCH_UP);
                while self.unsimulated >= 1.0 / TICK_RATE {
                    self.unsimulated -= 1.0 / TICK_RATE;
                    self.ticks += 1;
                    for player in &mut self.players {
                        wander(player, &self.world, self.camera.position);
                    }
                }
                let alpha = self.unsimulated * TICK_RATE;
                let avatars: Vec<Avatar> = self.players.iter().map(|p| p.avatar(alpha)).collect();
                let day_fraction = ((self.ticks % DAY_TICKS) as f32 + alpha) / DAY_TICKS as f32;
                renderer.render(
                    self.camera.view_projection(renderer.aspect_ratio()),
                    &Sky::at(day_fraction),
                    day_fraction * DAY_TICKS as f32 / TICK_RATE,
                    &avatars,
                );
                window.request_redraw();
            }
            _ => {}
        }
    }

    fn device_event(&mut self, _event_loop: &ActiveEventLoop, _id: DeviceId, event: DeviceEvent) {
        // Raw mouse motion keeps turning even when the locked cursor can't move.
        if let DeviceEvent::MouseMotion { delta: (dx, dy) } = event
            && self.mouse_captured
        {
            self.spectator.look(&mut self.camera, dx, dy);
        }
    }
}

/// Click to capture the mouse for looking around, Escape to give it back. Returns whether the
/// mouse is now captured.
fn set_mouse_capture(window: &Window, capture: bool) -> bool {
    if !capture {
        let _ = window.set_cursor_grab(CursorGrabMode::None);
        window.set_cursor_visible(true);
        return false;
    }
    // Locked is the natural mode but Windows only supports Confined, and some platforms only Locked.
    let grabbed = window
        .set_cursor_grab(CursorGrabMode::Locked)
        .or_else(|_| window.set_cursor_grab(CursorGrabMode::Confined))
        .is_ok();
    window.set_cursor_visible(!grabbed);
    grabbed
}

/// 4x2x4 chunks: 64 blocks square and 32 tall, with walls tall enough to reach the upper chunks.
const WORLD_CHUNKS: IVec3 = IVec3::new(4, 2, 4);
const GROUND_HEIGHT: i32 = 8;
const WALL_HEIGHT: i32 = 20;

/// The middle of the world at ground level; the camera starts out looking at it.
const CENTRE: Vec3 = vec3(32.0, GROUND_HEIGHT as f32, 32.0);

const MAX_CATCH_UP: f32 = 0.25;

/// The world starts mid-morning.
const START_TICKS: u64 = DAY_TICKS * 3 / 8;

/// Four players near the centre, one in each skin: one dropped from twelve blocks up and one on
/// the ground, both standing idle, and two walkers, one of which starts by walking into the centre
/// pit and jumping out, looking down and to its right.
fn test_players() -> Vec<Player> {
    let ground = GROUND_HEIGHT as f32;
    let dropped = Player::new(
        vec3(37.5, ground + 12.0, 27.5),
        -135f32.to_radians(),
        Skin::Player,
    );
    let idler = Player::new(vec3(35.5, ground, 31.5), -135f32.to_radians(), Skin::Autumn);
    let mut walker = Player::new(vec3(39.5, ground, 25.5), 123f32.to_radians(), Skin::CyberA);
    walker.controls.head_yaw = -0.35;
    walker.controls.head_pitch = -0.3;
    walker.controls.walk = Vec2::Y;
    let mut stroller = Player::new(vec3(30.5, ground, 38.5), -100f32.to_radians(), Skin::CyberB);
    stroller.controls.walk = Vec2::Y;
    vec![dropped, idler, walker, stroller]
}

/// Stand-in agent: drives a player only through its controls, as a real agent will. Walkers walk
/// forward, jump at anything in the way, and turn away if it's still blocking on the way back
/// down. Everyone else stands still and watches the spectator's camera when it comes close.
fn wander(player: &mut Player, world: &World, camera: Vec3) {
    let blocked = player.tick(world);
    if player.controls.walk == Vec2::ZERO {
        look_toward(player, camera);
    } else if blocked.x || blocked.z {
        if player.is_on_ground() {
            player.controls.jump = Some(*JUMP_HEIGHTS.end());
        } else if player.vertical_velocity() < 0.0 {
            player.controls.yaw += 100f32.to_radians();
        }
    }
}

/// Blocks from the feet to the eyes.
const EYE_HEIGHT: f32 = 1.62;
/// How close the camera must come before an idle player looks at it.
const LOOK_RANGE: f32 = 8.0;
/// How far the head turns from the body, side to side and up and down.
const HEAD_YAW_LIMIT: f32 = 70f32.to_radians();
const HEAD_PITCH_LIMIT: f32 = 45f32.to_radians();
/// Fraction of the remaining turn the head makes each tick.
const HEAD_EASE: f32 = 0.3;

/// Turns the head toward `target` as far as the neck allows while it's within `LOOK_RANGE`, and
/// back to straight ahead otherwise.
fn look_toward(player: &mut Player, target: Vec3) {
    let to_target = target - (player.avatar(1.0).position + Vec3::Y * EYE_HEIGHT);
    let (yaw, pitch) = if to_target.length() < LOOK_RANGE {
        let yaw = (-to_target.x).atan2(-to_target.z) - player.controls.yaw;
        let yaw = (yaw + PI).rem_euclid(TAU) - PI;
        let pitch = to_target.y.atan2(to_target.with_y(0.0).length());
        (
            yaw.clamp(-HEAD_YAW_LIMIT, HEAD_YAW_LIMIT),
            pitch.clamp(-HEAD_PITCH_LIMIT, HEAD_PITCH_LIMIT),
        )
    } else {
        (0.0, 0.0)
    };
    let controls = &mut player.controls;
    controls.head_yaw += (yaw - controls.head_yaw) * HEAD_EASE;
    controls.head_pitch += (pitch - controls.head_pitch) * HEAD_EASE;
}

fn main() {
    env_logger::init();
    let event_loop = EventLoop::new().expect("create event loop");
    let mut app = App {
        world: world::arena::generate(WORLD_CHUNKS, GROUND_HEIGHT, WALL_HEIGHT),
        players: test_players(),
        ticks: START_TICKS,
        unsimulated: 0.0,
        window: None,
        renderer: None,
        camera: Camera::looking_at(CENTRE + vec3(40.0, 25.0, 40.0), CENTRE),
        spectator: Spectator::default(),
        mouse_captured: false,
        last_frame: Instant::now(),
    };
    event_loop.run_app(&mut app).expect("run event loop");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wanderer_stays_inside_the_arena_walls() {
        let world = world::arena::generate(WORLD_CHUNKS, GROUND_HEIGHT, WALL_HEIGHT);
        let mut players = test_players();
        let inside = (WORLD_CHUNKS * world::CHUNK_SIZE as i32).as_vec3() - 1.0;
        // An hour of simulation.
        for _ in 0..(3600.0 * TICK_RATE) as u32 {
            for player in &mut players {
                wander(player, &world, CENTRE);
                let position = player.avatar(1.0).position;
                assert!(position.x > 1.0 && position.z > 1.0, "{position}");
                assert!(position.x < inside.x && position.z < inside.z, "{position}");
            }
        }
    }

    #[test]
    fn idle_player_looks_at_a_nearby_camera_within_its_neck_limits() {
        let mut player = Player::new(Vec3::ZERO, 0.0, Skin::Player);
        // Up and to the left of a player facing -Z.
        let camera = vec3(-2.0, EYE_HEIGHT + 2.0, -2.0);
        for _ in 0..50 {
            look_toward(&mut player, camera);
        }
        let controls = &player.controls;
        assert!((controls.head_yaw - 45f32.to_radians()).abs() < 1e-3);
        assert!(controls.head_pitch > 0.5 && controls.head_pitch <= HEAD_PITCH_LIMIT);

        // Directly behind: the head turns as far as it can.
        for _ in 0..50 {
            look_toward(&mut player, vec3(0.5, EYE_HEIGHT, 3.0));
        }
        assert!((player.controls.head_yaw.abs() - HEAD_YAW_LIMIT).abs() < 1e-3);

        // Out of range: back to straight ahead.
        for _ in 0..50 {
            look_toward(&mut player, vec3(0.0, EYE_HEIGHT, -LOOK_RANGE * 2.0));
        }
        assert!(player.controls.head_yaw.abs() < 1e-3);
    }
}

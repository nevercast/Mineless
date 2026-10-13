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

use glam::{IVec2, IVec3, Vec2, Vec3, vec3};
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
    timings: Timings,
}

/// Frame rate, draw time and tick time over about a second, then shown in the window title so
/// performance changes have numbers to compare. Draw time leaves out waiting for vsync.
#[derive(Default)]
struct Timings {
    frames: u32,
    frame_seconds: f32,
    draws: u32,
    draw_seconds: f32,
    worst_draw_seconds: f32,
    ticks: u32,
    tick_seconds: f32,
}

impl Timings {
    fn title(&self) -> String {
        let ms = |seconds: f32, count: u32| seconds * 1000.0 / count.max(1) as f32;
        format!(
            "Mineless | {:.0} fps | draw {:.1} ms, worst {:.1} ms | tick {:.2} ms",
            self.frames as f32 / self.frame_seconds,
            ms(self.draw_seconds, self.draws),
            ms(self.worst_draw_seconds, 1),
            ms(self.tick_seconds, self.ticks),
        )
    }
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
                    let tick_start = Instant::now();
                    for player in &mut self.players {
                        wander(player, &self.world, self.camera.position);
                    }
                    self.timings.ticks += 1;
                    self.timings.tick_seconds += tick_start.elapsed().as_secs_f32();
                }
                let alpha = self.unsimulated * TICK_RATE;
                let avatars: Vec<Avatar> = self.players.iter().map(|p| p.avatar(alpha)).collect();
                let day_fraction = ((self.ticks % DAY_TICKS) as f32 + alpha) / DAY_TICKS as f32;
                let draw = renderer.render(
                    &self.camera,
                    &Sky::at(day_fraction),
                    day_fraction * DAY_TICKS as f32 / TICK_RATE,
                    &avatars,
                );
                let timings = &mut self.timings;
                timings.frames += 1;
                timings.frame_seconds += seconds;
                if let Some(draw) = draw {
                    let draw = draw.as_secs_f32();
                    timings.draws += 1;
                    timings.draw_seconds += draw;
                    timings.worst_draw_seconds = timings.worst_draw_seconds.max(draw);
                }
                if timings.frame_seconds >= 1.0 {
                    window.set_title(&timings.title());
                    *timings = Timings::default();
                }
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

/// 20x8x20 chunks: 320 blocks square, big enough that render costs show up, and 128 tall, room
/// for the highest mountains.
const WORLD_CHUNKS: IVec3 = IVec3::new(20, 8, 20);
/// Fixed for now so every run, and every before-and-after measurement, sees the same world.
const SEED: u32 = 0;

/// The top of the ground at the centre of a column, where a player can stand.
fn ground_at(world: &World, x: i32, z: i32) -> Vec3 {
    vec3(
        x as f32 + 0.5,
        world.column_top(x, z) as f32,
        z as f32 + 0.5,
    )
}

const MAX_CATCH_UP: f32 = 0.25;

/// The world starts mid-morning.
const START_TICKS: u64 = DAY_TICKS * 3 / 8;

/// Blocks between neighbouring players in the starting grid.
const PLAYER_SPACING: i32 = 64;

/// A player every `PLAYER_SPACING` blocks across the world, skins in turn, so ticks have a crowd
/// to simulate. The first stands idle, the second drops from twelve blocks up and then stands
/// idle too, and the rest walk.
fn test_players(world: &World) -> Vec<Player> {
    let across = WORLD_CHUNKS * world::CHUNK_SIZE as i32 / PLAYER_SPACING;
    (0..across.x)
        .flat_map(|x| (0..across.z).map(move |z| IVec2::new(x, z)))
        .enumerate()
        .map(|(index, cell)| {
            let column = cell * PLAYER_SPACING + PLAYER_SPACING / 2;
            let mut position = ground_at(world, column.x, column.y);
            if index == 1 {
                position.y += 12.0;
            }
            let yaw = (index as f32 * 123.0).to_radians();
            let mut player = Player::new(position, yaw, Skin::ALL[index % Skin::ALL.len()]);
            if index > 1 {
                player.controls.walk = Vec2::Y;
            }
            player
        })
        .collect()
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
    let world = world::terrain::generate(WORLD_CHUNKS, SEED);
    let mut app = App {
        players: test_players(&world),
        world,
        ticks: START_TICKS,
        unsimulated: 0.0,
        window: None,
        renderer: None,
        // Over the plains, looking at the escarpment where the eastern upland rises out of them.
        camera: Camera::looking_at(vec3(190.0, 72.0, 110.0), vec3(280.0, 68.0, 95.0)),
        spectator: Spectator::default(),
        mouse_captured: false,
        last_frame: Instant::now(),
        timings: Timings::default(),
    };
    event_loop.run_app(&mut app).expect("run event loop");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wanderer_stays_inside_the_world_walls() {
        let world = world::terrain::generate(WORLD_CHUNKS, SEED);
        let mut players = test_players(&world);
        let inside = (WORLD_CHUNKS * world::CHUNK_SIZE as i32).as_vec3() - 1.0;
        let camera = ground_at(&world, 160, 160);
        // An hour of simulation.
        for _ in 0..(3600.0 * TICK_RATE) as u32 {
            for player in &mut players {
                wander(player, &world, camera);
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

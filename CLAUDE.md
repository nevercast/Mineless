# Mineless

Agent-first Minecraft-like voxel game in Rust (wgpu + winit). Agents are the players; the human
mostly spectates.

## Rules
- Smallest step that delivers the goal. One step per PR.
- No speculative abstraction: no trait, generic, config or module until a second real use exists.
- Big-picture architecture and design belong to Josh. Propose, don't decide: new crates, new
  threads/async, public data formats, and anything that shapes the agent API.
- Josh approves every new dependency. Ask before adding one to Cargo.toml.
- Comments explain why, never what. No commented-out code, no TODOs without an owner.
- No markdown docs in the repo beyond this file, REVIEW.md and README.
- Must build and run on Windows and Linux. `cargo fmt --all`, `cargo clippy --workspace
  --all-targets` and `cargo test --workspace` must pass with warnings as errors before pushing.
- No `unwrap()` outside tests (clippy enforces it). `expect` must say why it can't fail.
- `crates/sim` holds the game state agents act on and must never depend on wgpu, winit or
  rendering concerns such as vertex layouts.
- Tests cover pure logic (sim, meshing); don't test GPU code.
- Screenshots taken during development go in the PR description, each with a description of
  what it depicts. Don't commit them.

## Layout
- `crates/sim`: voxel world, physics, players on a 20 Hz fixed tick, and the avatar pose and skin.
- `src/`: the app. `mesher.rs` and `avatar.rs` turn sim state into vertices, `render/` is the GPU
  side, `main.rs` owns the window and the tick loop.

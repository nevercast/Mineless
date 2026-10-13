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
- Must build and run on Windows and Linux. `cargo fmt`, `cargo clippy --all-targets` and
  `cargo test` must pass with warnings as errors before pushing.
- No `unwrap()` outside tests (clippy enforces it). `expect` must say why it can't fail.
- Keep simulation code (world, mesher, physics) free of wgpu and winit.
- Tests cover pure logic (physics, meshing, world); don't test GPU code.
- Screenshots taken during development go in the PR description, each with a description of
  what it depicts. Don't commit them.

## Layout
- `world/` voxel data, `mesher.rs` CPU meshing, `physics.rs` 20 Hz fixed-tick sim,
  `render/` GPU side only.

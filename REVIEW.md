# Review checklist

Flag as blocking:
- A new dependency in Cargo.toml without Josh's approval in the PR.
- Abstraction with a single use (trait, generic, builder, config struct, wrapper type).
- Dead code, unused parameters, `pub` items only used inside their module.
- Comments that restate the code, or doc comments on obvious items.
- `expect` whose message doesn't say why failure can't happen.
- Simulation modules importing wgpu or winit.
- Platform-specific code without a Windows path.
- A change wider than the PR's stated step.
- Screenshots committed to the tree, or a PR with screenshots that lack a description.

Flag as optional:
- Naming that doesn't match nearby code.
- A function that would read better split or inlined.

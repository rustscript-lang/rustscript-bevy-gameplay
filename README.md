# rustscript-bevy-gameplay

Standalone Bevy integration demo for `pd-vm` / RustScript.

## Screenshots

![RustScript Bevy Shooter](docs/screenshots/shooter.png)

![RustScript Gomoku](docs/screenshots/gomoku.png)

![RustScript Xiangqi](docs/screenshots/xiangqi.png)

## What it proves

This repo demonstrates three playable Bevy examples whose gameplay rules are driven by live RustScript:

- **Shooter**: a vertical scrolling shooter with textured ships, enemy waves, rewards, player health, different projectile patterns, missiles, shockwaves, pause/restart controls, and a live script panel that can change the running world without recreating spawned entities.
- **Gomoku**: a human-vs-AI board game where move legality, win detection, and AI move selection are implemented in RustScript. The UI supports live editing, save/load of board state plus scripts, AI assist, AI bias, JIT trace telemetry, and debugger controls.
- **Xiangqi**: a Chinese chess game with board rendering, piece artwork, scripted legal-move validation, scripted AI move selection, save/load, AI assist, AI bias, JIT telemetry, and the same live debugging workflow.

Across the examples, Bevy keeps the rendering and ECS shell compiled while RustScript owns the parts that are useful to tune during development: gameplay rules, AI behavior, spawn schedules, rewards, and balancing constants. The editor can lint scripts as you type, apply changes after a short cooldown, reset to embedded defaults, pause in a debugger, step through code, inspect locals, use breakpoints, and interact through the debug console. The VM runs with JIT enabled and exposes trace counts in the game UI so script performance work is visible while playing.

Assets and scripts are embedded into the binaries, so release packages do not need external `assets/` or `scripts/` directories.

## Run

```bash
cargo test --tests
cargo run --example combat
cargo run --example shooter
cargo run --example gomoku
cargo run --example xiangqi
```

`combat` is a small headless ECS script demo. The other three examples open Bevy windows with the live RustScript editor docked on the right.

Perf-oriented AI checks are marked as ignored tests:

```bash
cargo test perf --tests -- --ignored
```

For headless smoke checks:

```bash
cargo run --example shooter -- --script-smoke
cargo run --example gomoku -- --script-smoke
cargo run --example xiangqi -- --script-smoke
```

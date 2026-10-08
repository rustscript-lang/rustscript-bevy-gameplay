# rustscript-bevy-gameplay

Standalone Bevy integration demo for `pd-vm` / RustScript.

Play in your browser: **[RustScript Arcade](https://bevy.rustscript.org/)** — [Shooter](https://bevy.rustscript.org/shooter/), [Gomoku](https://bevy.rustscript.org/gomoku/), [Xiangqi](https://bevy.rustscript.org/xiangqi/).

RustScript core is pinned to `805991cfc6d81e7b9d6c042a222ecf70e3f2dab0` through Git dependencies. This is the latest host-descriptor integration branch revision, compatible with the upstream gameplay migration; core master currently has a different host API. A sibling core checkout is no longer required.

## Screenshots

![RustScript Bevy Shooter](docs/screenshots/shooter.png)

![RustScript Gomoku](docs/screenshots/gomoku.png)

![RustScript Xiangqi](docs/screenshots/xiangqi.png)

## What it proves

This repo demonstrates three playable Bevy examples whose gameplay rules are driven by live RustScript:

- **Shooter**: a vertical scrolling shooter whose movement, aircraft behavior, fire patterns, projectile physics, homing, shockwaves, collisions, rewards, scoring, game-over rules, and spawn timers run in RSS. Its five-tab live editor shares Gomoku's linting, automatic apply, breakpoints, stepping, locals, hover inspection, and debug console.
- **Gomoku**: a human-vs-AI board game where move legality, win detection, and AI move selection are implemented in RustScript. The UI supports live editing, save/load of board state plus scripts, AI assist, AI bias, JIT trace telemetry, and debugger controls.
- **Xiangqi**: a Chinese chess game with board rendering, piece artwork, scripted legal-move validation, scripted AI move selection, save/load, AI assist, AI bias, JIT telemetry, and the same live debugging workflow.

Across the examples, Bevy keeps the rendering and ECS shell compiled while RustScript owns the parts that are useful to tune during development: gameplay rules, AI behavior, spawn schedules, rewards, and balancing constants. The editor can lint scripts as you type, apply changes after a short cooldown, reset to embedded defaults, pause in a debugger, step through code, inspect locals, use breakpoints, and interact through the debug console. The VM runs with JIT enabled and exposes trace counts in the game UI so script performance work is visible while playing.

Assets and scripts are embedded into the binaries, so release packages do not need external `assets/` or `scripts/` directories.

Shooter's editable scripts are:

| File | Responsibility |
| --- | --- |
| `shooter_game.rss` | Initial loadout, enemy wave, rewards, spawn-rule registration |
| `shooter_flow.rss` | Player movement, pickup effects and health/ammunition limits |
| `shooter_planes.rss` | Aircraft trajectories, enemy power and fire clocks |
| `shooter_projectiles.rss` | Eight projectile types, firing patterns, guidance, lifetime, collision, damage, drops and score |
| `shooter_spawns.rss` | Repeating timers and one-shot kill thresholds |

Rust provides ECS data access, input, rendering and VM invocation. Frame scripts reuse compiled VMs and JIT traces. Edits with lint errors retain the previous working source; a runtime error pauses gameplay and reports the affected tab. Initialization edits apply to the live world, while Restart clears runtime state and reapplies the initialization script.

On desktop or in the browser, select any Shooter tab and press **Debug**. Gameplay pauses while that script runs against a snapshot of the current ECS world. **Step**, **Next**, **Continue**, **Locals**, gutter breakpoints and the console work through the shared debugger bridge. **Stop** releases the debug session and restores the prior gameplay state. Debug evaluation preserves the live world. The browser debugger advances in bounded VM slices so the editor remains responsive.

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

## Web / WebAssembly

The same three Bevy examples compile to `wasm32-unknown-unknown`, with WebGL2 rendering and RustScript interpreter execution. Native builds retain Cranelift JIT. Live script editing, rule validation, AI, undo/redo, and restart are available in the browser. Board Save/Load uses local storage scoped to each game and browser origin. Browser debugging uses resumable VM execution with a per-frame budget. Use Debug, Step, Next, Out, Continue, Locals, line breakpoints, and the debug console in the script panel. All debug sessions evaluate a snapshot of the current gameplay state. Shooter supports all five RSS tabs and restores the prior gameplay state after completion or Stop. Board AI Debug arms the next AI turn; the selected move is applied after the session completes. Pause/Stop remain responsive during long scripts. Native builds retain the thread-based debugger.

Prerequisites: Rust, Python 3.11+, and a `wasm-bindgen-cli` version matching `wasm-bindgen` in `Cargo.lock` (currently `0.2.126`).

```bash
rustup target add wasm32-unknown-unknown
cargo install wasm-bindgen-cli --version 0.2.126 --locked
python tools/build_web.py
python -m http.server 8000 --directory dist/web
```

Open `http://localhost:8000`. Use a desktop browser with WebGL2 enabled. Each game has loading progress, retry, fit-to-window, original-size, and fullscreen controls. Shooter uses WASD or arrow keys and fires automatically. Board games use pointer input. AI executes synchronously, so a complex move can briefly delay rendering in interpreter mode.

The `Pages` workflow builds all three examples and deploys `dist/web` on pushes to `master`, or through manual dispatch. Configure the repository's Pages source as **GitHub Actions**, with the custom domain `bevy.rustscript.org`. The build copies `web/CNAME` into the published site; its DNS CNAME points to `rustscript-lang.github.io`. All resource URLs are relative so the site also works under the repository's Pages subpath.

Xiangqi embeds a small Noto Sans CJK subset for its Chinese labels. The font is licensed under SIL OFL 1.1; see `assets/fonts/LICENSE.txt`.

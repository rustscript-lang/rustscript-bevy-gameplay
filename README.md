# rustscript-bevy-gameplay

Standalone Bevy ECS integration demo for `pd-vm` / RustScript.

## What it proves

A game can keep Bevy ECS systems and components compiled while moving balance logic into RustScript:

- compiled components: `Health`, `Armor`
- compiled system boundary: `apply_scripted_damage(&mut World, Entity, incoming, critical)`
- RustScript calls `bevy_armor() -> int`, a host function backed by the Bevy ECS component value on the target entity
- scripted behavior: damage mitigation and critical-hit formula in `scripts/damage_formula.rss`

This does not fork or patch Bevy. It depends on upstream `bevy_ecs` and local `pd-vm` path only.

## Run

```bash
cargo test --tests --jobs 4
cargo run --example combat
```

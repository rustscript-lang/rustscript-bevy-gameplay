# rustscript-bevy-gameplay

Standalone Bevy ECS integration demo for `pd-vm` / RustScript.

## What it proves

A game can keep Bevy ECS systems and components compiled while moving balance logic into RustScript:

- compiled components: `Health`, `Armor`
- compiled system boundary: `apply_scripted_damage(&mut World, Entity, incoming, critical)`
- RustScript calls namespaced Bevy host functions: `bevy::World::contains_entity`, `bevy::World::get_health`, `bevy::World::get_armor`, `bevy::World::set_health`
- host functions are exported with `#[pd_host_function(name = "bevy::World::...")]`; the `.rss` names and namespaces match the bound host names exactly

This does not fork or patch Bevy. It depends on upstream `bevy_ecs` plus local `pd-vm` / `pd-host-function` paths only.

## Run

```bash
cargo test --tests --jobs 4
cargo run --example combat
```

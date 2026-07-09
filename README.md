# rustscript-bevy-gameplay

Standalone Bevy integration demo for `pd-vm` / RustScript.

## What it proves

A game can keep Bevy ECS systems and rendering compiled while moving gameplay tuning into RustScript:

- compiled combat components: `Health`, `Armor`, `Player`, `Enemy`, `Position`, attack components
- compiled system boundary: `apply_scripted_damage(&mut World, Entity, incoming, critical)`
- live shooter configuration: `apply_shooter_script(&mut World, source)` updates the same Bevy world in place
- RustScript calls namespaced Bevy host functions:
  - `bevy::World::contains_entity`
  - `bevy::World::get_health`
  - `bevy::World::get_armor`
  - `bevy::World::set_health`
  - `bevy::Shooter::set_player_health`
  - `bevy::Shooter::set_player_attack`
  - `bevy::Shooter::spawn_enemy`
- host functions are exported with `#[pd_host_function(name = "...")]`; the `.rss` names and namespaces match the bound host names exactly

This uses upstream Bevy crates plus local `pd-vm` / `pd-host-function` paths.

## Run

```bash
cargo test --tests --jobs 4
cargo run --example combat
cargo run --example shooter
```

`cargo run --example shooter` opens a simple side-scrolling flight shooter. The right-side panel contains the currently active RustScript. Change player health, attack style/power/cooldown, or the enemy wave, then click **Save and apply now**; the existing game world updates in place.

For headless CI smoke:

```bash
cargo run --example shooter -- --script-smoke
```

use bevy_ecs::prelude::*;
use pretty_assertions::assert_eq;
use rustscript_bevy_gameplay::{
    AttackPower, AttackStyle, Enemy, Health, Player, Position, apply_shooter_script,
};

#[test]
fn rustscript_spawns_player_and_enemy_waves() {
    let mut world = World::new();

    let summary = apply_shooter_script(&mut world, include_str!("../scripts/shooter_game.rss"))
        .expect("shooter script should apply");

    assert_eq!(summary.player_health, 120);
    assert_eq!(summary.player_attack_style, "spread");
    assert_eq!(summary.player_attack_power, 14);
    assert_eq!(summary.enemies_spawned, 4);

    let (_, health, style, power) = world
        .query::<(&Player, &Health, &AttackStyle, &AttackPower)>()
        .single(&world)
        .expect("player should exist");
    assert_eq!(health.0, 120);
    assert_eq!(style.0, "spread");
    assert_eq!(power.0, 14);

    let mut enemy_query = world.query::<(&Enemy, &Health, &AttackStyle, &Position)>();
    let enemies = enemy_query.iter(&world).collect::<Vec<_>>();
    assert_eq!(enemies.len(), 4);
    assert!(enemies.iter().any(|(enemy, health, style, position)| {
        enemy.kind == "bomber" && health.0 == 42 && style.0 == "burst" && position.x == 720.0
    }));
}

#[test]
fn reapplying_script_updates_live_world_without_recreating_player() {
    let mut world = World::new();
    apply_shooter_script(&mut world, include_str!("../scripts/shooter_game.rss"))
        .expect("initial script should apply");
    let player_before = world
        .query::<(Entity, &Player)>()
        .single(&world)
        .expect("player should exist")
        .0;

    let updated = r#"
use bevy;
let hp: bool = bevy::Shooter::set_player_health(77);
let attack: bool = bevy::Shooter::set_player_attack("laser", 31, 90);
let enemy: bool = bevy::Shooter::spawn_enemy("ace", 55, "wave", 740, 0);
true;
"#;
    let summary = apply_shooter_script(&mut world, updated).expect("updated script should apply");

    let (player_after, _, health, style, power) = world
        .query::<(Entity, &Player, &Health, &AttackStyle, &AttackPower)>()
        .single(&world)
        .expect("player should still exist");
    assert_eq!(player_after, player_before);
    assert_eq!(health.0, 77);
    assert_eq!(style.0, "laser");
    assert_eq!(power.0, 31);
    assert_eq!(summary.enemies_spawned, 1);

    let mut enemies = world.query::<(&Enemy, &Health, &AttackStyle)>();
    let spawned = enemies.iter(&world).collect::<Vec<_>>();
    assert_eq!(spawned.len(), 1);
    assert_eq!(spawned[0].0.kind, "ace");
    assert_eq!(spawned[0].1.0, 55);
    assert_eq!(spawned[0].2.0, "wave");
}

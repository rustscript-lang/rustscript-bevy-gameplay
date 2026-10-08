use bevy_ecs::prelude::*;
use rustscript_bevy_gameplay::*;

const FLOW: &str = include_str!("../scripts/shooter_flow.rss");
const PLANES: &str = include_str!("../scripts/shooter_planes.rss");
const PROJECTILES: &str = include_str!("../scripts/shooter_projectiles.rss");

fn frame(world: &mut World, delta: f64) {
    world
        .get_resource_or_insert_with(ShooterFrame::default)
        .0
        .insert("delta_ms".into(), delta);
    for source in [FLOW, PLANES, PROJECTILES] {
        run_shooter_frame_script(world, source).unwrap();
    }
}

#[test]
fn rss_moves_ships_and_fires_all_projectile_kinds() {
    for kind in [
        "bolt",
        "spread",
        "laser",
        "missile",
        "shockwave",
        "plasma",
        "flak",
        "rail",
    ] {
        let mut world = World::new();
        apply_shooter_script(
            &mut world,
            &format!("use bevy; bevy::Shooter::set_player_projectiles(\"{kind}\", 3); true;"),
        )
        .unwrap();
        world
            .get_resource_or_insert_with(ShooterFrame::default)
            .0
            .insert("input_x".into(), 1.0);
        let player = world
            .query_filtered::<Entity, With<Player>>()
            .single(&world)
            .unwrap();
        let before = world.get::<Position>(player).unwrap().x;
        frame(&mut world, 300.0);
        assert!(world.get::<Position>(player).unwrap().x > before);
        assert_eq!(
            world.query::<&ShooterProjectile>().iter(&world).count(),
            3,
            "{kind}"
        );
    }
}

#[test]
fn rss_owns_damage_drops_score_and_reward_caps() {
    let mut world = World::new();
    apply_shooter_script(&mut world, "use bevy; bevy::Shooter::set_player_health(120); bevy::Shooter::spawn_enemy(\"tank\", 1, \"straight\", 0, -150); true;").unwrap();
    frame(&mut world, 300.0);
    assert_eq!(world.query::<&Enemy>().iter(&world).count(), 0);
    assert_eq!(world.resource::<ShooterFrame>().0["score"], 1.0);
    assert_eq!(
        world.query::<&RewardItem>().single(&world).unwrap().amount,
        20
    );
    let player = world
        .query_filtered::<Entity, With<Player>>()
        .single(&world)
        .unwrap();
    let pos = *world
        .query_filtered::<&Position, With<RewardItem>>()
        .single(&world)
        .unwrap();
    *world.get_mut::<Position>(player).unwrap() = pos;
    world.get_mut::<Health>(player).unwrap().0 = 115;
    run_shooter_frame_script(&mut world, FLOW).unwrap();
    assert_eq!(world.get::<Health>(player).unwrap().0, 120);
    assert_eq!(world.query::<&RewardItem>().iter(&world).count(), 0);
}

fn player_world(kind: &str) -> World {
    let mut world = World::new();
    apply_shooter_script(
        &mut world,
        &format!(
            r#"use bevy;
        bevy::Shooter::set_player_health(120);
        bevy::Shooter::set_player_attack("straight", 8, 260);
        bevy::Shooter::set_player_projectiles("{kind}", 1); true;"#
        ),
    )
    .unwrap();
    world.insert_resource(ShooterFrame::default());
    world
}

fn fire_once(world: &mut World, delta: f64) {
    let player = world
        .query_filtered::<Entity, With<Player>>()
        .single(world)
        .unwrap();
    world
        .entity_mut(player)
        .insert(ShooterData(std::collections::HashMap::from([(
            "fire".into(),
            1.0,
        )])));
    world
        .resource_mut::<ShooterFrame>()
        .0
        .insert("delta_ms".into(), delta);
    run_shooter_frame_script(world, PROJECTILES).unwrap();
}

#[test]
fn missiles_turn_toward_nearest_target_without_reversing_forward_axis() {
    let mut world = player_world("missile");
    apply_shooter_script(
        &mut world,
        "use bevy; bevy::Shooter::spawn_enemy(\"scout\", 99, \"straight\", 100, -450); true;",
    )
    .unwrap();
    fire_once(&mut world, 50.0);
    let velocity = world
        .query_filtered::<&Velocity, With<ShooterProjectile>>()
        .single(&world)
        .unwrap();
    assert!(velocity.x > 0.0);
    assert!(
        velocity.y > 0.0,
        "target behind missile must not reverse it"
    );
}

#[test]
fn shockwaves_expand_hit_each_target_once_and_expire() {
    let mut world = player_world("shockwave");
    apply_shooter_script(
        &mut world,
        "use bevy; bevy::Shooter::spawn_enemy(\"tank\", 100, \"straight\", 60, -338); true;",
    )
    .unwrap();
    fire_once(&mut world, 0.0);
    let enemy = world
        .query_filtered::<Entity, With<Enemy>>()
        .single(&world)
        .unwrap();
    assert_eq!(world.get::<Health>(enemy).unwrap().0, 100);
    world
        .resource_mut::<ShooterFrame>()
        .0
        .insert("delta_ms".into(), 350.0);
    run_shooter_frame_script(&mut world, PROJECTILES).unwrap();
    assert_eq!(world.get::<Health>(enemy).unwrap().0, 96);
    world
        .resource_mut::<ShooterFrame>()
        .0
        .insert("delta_ms".into(), 200.0);
    run_shooter_frame_script(&mut world, PROJECTILES).unwrap();
    assert_eq!(world.get::<Health>(enemy).unwrap().0, 96);
    run_shooter_frame_script(&mut world, PROJECTILES).unwrap();
    assert_eq!(world.query::<&ShooterProjectile>().iter(&world).count(), 0);
}

#[test]
fn piercing_shots_destroy_multiple_targets_and_count_each_kill_once() {
    let mut world = player_world("laser");
    apply_shooter_script(
        &mut world,
        r#"use bevy;
        bevy::Shooter::spawn_enemy("scout", 10, "straight", 0, -322);
        bevy::Shooter::spawn_enemy("carrier", 10, "burst", 0, -322); true;"#,
    )
    .unwrap();
    fire_once(&mut world, 0.0);
    assert_eq!(world.query::<&Enemy>().iter(&world).count(), 0);
    assert_eq!(world.resource::<ShooterFrame>().0["score"], 2.0);
    assert_eq!(world.query::<&RewardItem>().iter(&world).count(), 2);
    assert_eq!(world.query::<&ShooterProjectile>().iter(&world).count(), 1);
    run_shooter_frame_script(&mut world, PROJECTILES).unwrap();
    assert_eq!(world.resource::<ShooterFrame>().0["score"], 2.0);
}

#[test]
fn enemy_types_have_distinct_motion_and_fire_patterns() {
    let mut velocities = Vec::new();
    for (kind, expected) in [
        ("sniper", vec!["rail"]),
        ("carrier", vec!["missile", "plasma"]),
        ("striker", vec!["flak", "flak", "flak"]),
    ] {
        let mut world = player_world("bolt");
        apply_shooter_script(
            &mut world,
            &format!(
                "use bevy; bevy::Shooter::spawn_enemy(\"{kind}\", 30, \"straight\", 0, 200); true;"
            ),
        )
        .unwrap();
        let enemy = world
            .query_filtered::<Entity, With<Enemy>>()
            .single(&world)
            .unwrap();
        world
            .entity_mut(enemy)
            .insert(ShooterData(std::collections::HashMap::from([(
                "fire_clock".into(),
                1999.0,
            )])));
        frame(&mut world, 1.0);
        velocities.push(world.get::<Velocity>(enemy).unwrap().y);
        let shots = world
            .query::<&ShooterProjectile>()
            .iter(&world)
            .map(|p| p.kind.as_str())
            .collect::<Vec<_>>();
        assert_eq!(shots, expected, "{kind}");
    }
    assert!(velocities[0] != velocities[1] && velocities[1] != velocities[2]);
}

#[test]
fn edited_rss_changes_live_movement_and_invalid_edits_report_errors() {
    let mut world = player_world("bolt");
    world
        .resource_mut::<ShooterFrame>()
        .0
        .insert("input_x".into(), 1.0);
    world
        .resource_mut::<ShooterFrame>()
        .0
        .insert("delta_ms".into(), 100.0);
    let player = world
        .query_filtered::<Entity, With<Player>>()
        .single(&world)
        .unwrap();
    run_shooter_frame_script(&mut world, FLOW).unwrap();
    assert_eq!(world.get::<Position>(player).unwrap().x, 30.0);
    run_shooter_frame_script(&mut world, &FLOW.replace("300.0", "600.0")).unwrap();
    assert_eq!(world.get::<Position>(player).unwrap().x, 90.0);
    assert!(run_shooter_frame_script(&mut world, "let broken = ;").is_err());
    assert!(world.get::<Player>(player).is_some());
}

#[test]
fn enemy_hits_set_game_over_and_health_does_not_become_negative() {
    let mut world = player_world("bolt");
    run_shooter_frame_script(
        &mut world,
        r#"use bevy;
        bevy::Shooter::projectile("enemy", "bolt", 0.0, -360.0, 0.0, 0.0, 150.0); true;"#,
    )
    .unwrap();
    run_shooter_frame_script(&mut world, PROJECTILES).unwrap();
    let hp = world
        .query_filtered::<&Health, With<Player>>()
        .single(&world)
        .unwrap();
    assert_eq!(hp.0, 0);
    assert_eq!(world.resource::<ShooterFrame>().0["game_over"], 1.0);
}

#[test]
fn spawn_timers_preserve_fractional_frame_time_and_reset_on_apply() {
    let mut world = player_world("bolt");
    let setup = r#"use bevy; bevy::Shooter::spawn_enemy_every("scout", 12, "straight", 0, 500, 250); true;"#;
    let spawn = include_str!("../scripts/shooter_spawns.rss");
    apply_shooter_script(&mut world, setup).unwrap();
    world
        .resource_mut::<ShooterFrame>()
        .0
        .insert("delta_ms".into(), 16.5);
    for _ in 0..15 {
        run_shooter_frame_script(&mut world, spawn).unwrap();
    }
    assert_eq!(world.query::<&Enemy>().iter(&world).count(), 0);
    run_shooter_frame_script(&mut world, spawn).unwrap();
    assert_eq!(world.query::<&Enemy>().iter(&world).count(), 1);
    apply_shooter_script(&mut world, setup).unwrap();
    for _ in 0..15 {
        run_shooter_frame_script(&mut world, spawn).unwrap();
    }
    assert_eq!(world.query::<&Enemy>().iter(&world).count(), 1);
}

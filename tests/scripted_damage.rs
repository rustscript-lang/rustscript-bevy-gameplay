use bevy_ecs::prelude::*;
use pretty_assertions::assert_eq;
use rustscript_bevy_gameplay::{Armor, DamageRules, Health, apply_scripted_damage};

#[test]
fn rustscript_damage_formula_updates_bevy_components() {
    let mut world = World::new();
    world.insert_resource(
        DamageRules::from_source(include_str!("../scripts/damage_formula.rss"))
            .expect("rules should compile"),
    );
    let entity = world.spawn((Health(30), Armor(4))).id();

    let applied =
        apply_scripted_damage(&mut world, entity, 10, false).expect("damage should apply");

    assert_eq!(applied, 6);
    assert_eq!(world.get::<Health>(entity).unwrap().0, 24);
}

#[test]
fn rustscript_can_change_static_gameplay_formula_for_critical_hits() {
    let mut world = World::new();
    world.insert_resource(
        DamageRules::from_source(include_str!("../scripts/damage_formula.rss"))
            .expect("rules should compile"),
    );
    let entity = world.spawn((Health(30), Armor(4))).id();

    let applied = apply_scripted_damage(&mut world, entity, 10, true).expect("damage should apply");

    assert_eq!(applied, 12);
    assert_eq!(world.get::<Health>(entity).unwrap().0, 18);
}

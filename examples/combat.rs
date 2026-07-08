use bevy_ecs::prelude::*;
use rustscript_bevy_gameplay::{Armor, DamageRules, Health, apply_scripted_damage};

fn main() {
    let mut world = World::new();
    world.insert_resource(
        DamageRules::from_source(include_str!("../scripts/damage_formula.rss"))
            .expect("rules should compile"),
    );
    let entity = world.spawn((Health(30), Armor(4))).id();
    let applied = apply_scripted_damage(&mut world, entity, 10, true).expect("damage should apply");
    println!(
        "applied={applied}, health={}",
        world.get::<Health>(entity).unwrap().0
    );
}

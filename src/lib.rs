use bevy_ecs::prelude::*;
use vm::{CallOutcome, CallReturn, HostFunction, Value, Vm, VmError, VmStatus, compile_source};

#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct Health(pub i64);

#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct Armor(pub i64);

#[derive(Resource, Debug, Clone)]
pub struct DamageRules {
    source: String,
}

impl DamageRules {
    pub fn from_source(source: impl Into<String>) -> Result<Self, String> {
        let source = source.into();
        evaluate_damage(&source, 1, 0, false)?;
        Ok(Self { source })
    }
}

pub fn apply_scripted_damage(
    world: &mut World,
    entity: Entity,
    incoming: i64,
    critical: bool,
) -> Result<i64, String> {
    let armor = world
        .get::<Armor>(entity)
        .ok_or_else(|| format!("entity {entity:?} is missing Armor"))?
        .0;
    let source = world
        .get_resource::<DamageRules>()
        .ok_or_else(|| "World is missing DamageRules resource".to_string())?
        .source
        .clone();
    let applied = evaluate_damage(&source, incoming, armor, critical)?;
    let mut health = world
        .get_mut::<Health>(entity)
        .ok_or_else(|| format!("entity {entity:?} is missing Health"))?;
    health.0 -= applied;
    Ok(applied)
}

fn evaluate_damage(source: &str, incoming: i64, armor: i64, critical: bool) -> Result<i64, String> {
    let wrapped = format!(
        "let incoming = {incoming};\nlet armor = {armor};\nlet critical = {};\n{source}",
        if critical { "true" } else { "false" }
    );
    match run_value(&wrapped)? {
        Value::Int(value) => Ok(value),
        other => Err(format!("script returned {other:?}; expected int")),
    }
}

struct DamageFloorHost;

impl HostFunction for DamageFloorHost {
    fn call(&mut self, _vm: &mut Vm, args: &[Value]) -> Result<CallOutcome, VmError> {
        match args {
            [Value::Int(value)] => Ok(CallOutcome::Return(CallReturn::one(Value::Int(
                (*value).max(1),
            )))),
            _ => Err(VmError::TypeMismatch("damage int")),
        }
    }
}

fn run_value(source: &str) -> Result<Value, String> {
    let compiled = compile_source(source).map_err(|err| err.to_string())?;
    let mut vm = Vm::new(compiled.program);
    vm.bind_function("damage_floor", Box::new(DamageFloorHost));
    let status = vm.run().map_err(|err| err.to_string())?;
    if status != VmStatus::Halted {
        return Err(format!("script did not halt: {status:?}"));
    }
    vm.stack()
        .last()
        .cloned()
        .ok_or_else(|| "script returned an empty stack".to_string())
}

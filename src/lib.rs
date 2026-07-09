use std::cell::RefCell;

use bevy_ecs::prelude::*;
pub(crate) use vm::Vm;
use vm::{CallOutcome, CallReturn, Value, VmError, VmResult, VmStatus, compile_source};

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
        compile_source(&format!(
            "let incoming = 0;\nlet critical = false;\n{source}"
        ))
        .map_err(|err| err.to_string())?;
        Ok(Self { source })
    }
}

pub fn apply_scripted_damage(
    world: &mut World,
    entity: Entity,
    incoming: i64,
    critical: bool,
) -> Result<i64, String> {
    let source = world
        .get_resource::<DamageRules>()
        .ok_or_else(|| "World is missing DamageRules resource".to_string())?
        .source
        .clone();
    evaluate_damage(&source, world, entity, incoming, critical)
}

fn evaluate_damage(
    source: &str,
    world: &mut World,
    entity: Entity,
    incoming: i64,
    critical: bool,
) -> Result<i64, String> {
    let wrapped = format!(
        "let incoming = {incoming};\nlet critical = {};\n{source}",
        if critical { "true" } else { "false" }
    );
    match with_bevy_context(world, entity, || run_value(&wrapped))? {
        Value::Int(value) => Ok(value),
        other => Err(format!("script returned {other:?}; expected int")),
    }
}

#[derive(Clone, Copy)]
struct BevyContext {
    world: *mut World,
    entity: Entity,
}

thread_local! {
    static BEVY_CONTEXT: RefCell<Option<BevyContext>> = const { RefCell::new(None) };
}

struct BevyContextGuard;

impl Drop for BevyContextGuard {
    fn drop(&mut self) {
        BEVY_CONTEXT.with(|slot| {
            *slot.borrow_mut() = None;
        });
    }
}

fn with_bevy_context<T>(
    world: &mut World,
    entity: Entity,
    f: impl FnOnce() -> Result<T, String>,
) -> Result<T, String> {
    BEVY_CONTEXT.with(|slot| {
        *slot.borrow_mut() = Some(BevyContext { world, entity });
    });
    let _guard = BevyContextGuard;
    f()
}

fn with_world<T>(f: impl FnOnce(&mut World, Entity) -> VmResult<T>) -> VmResult<T> {
    BEVY_CONTEXT.with(|slot| {
        let ctx = slot
            .borrow()
            .ok_or_else(|| VmError::HostError("missing Bevy World context".to_string()))?;
        // SAFETY: the pointer is installed only for the synchronous VM run in apply_scripted_damage.
        unsafe { f(&mut *ctx.world, ctx.entity) }
    })
}

fn run_value(source: &str) -> Result<Value, String> {
    let compiled = compile_source(source).map_err(|err| err.to_string())?;
    let mut vm = Vm::new(compiled.program);
    bind_bevy_hosts(&mut vm);
    let status = vm.run().map_err(|err| err.to_string())?;
    if status != VmStatus::Halted {
        return Err(format!("script did not halt: {status:?}"));
    }
    vm.stack()
        .last()
        .cloned()
        .ok_or_else(|| "script returned an empty stack".to_string())
}

fn bind_bevy_hosts(vm: &mut Vm) {
    vm.bind_static_args_function(
        "bevy::World::contains_entity",
        host::bevy::world_contains_entity_host,
    );
    vm.bind_static_args_function("bevy::World::get_health", host::bevy::world_get_health_host);
    vm.bind_static_args_function("bevy::World::get_armor", host::bevy::world_get_armor_host);
    vm.bind_static_args_function("bevy::World::set_health", host::bevy::world_set_health_host);
}

mod host {
    use super::*;
    use pd_host_function::pd_host_function;

    pub(super) trait BorrowVmValue<'a>: Sized {
        fn borrow_vm_value(value: &'a Value, label: &str) -> VmResult<Self>;
    }

    pub(super) fn borrow_arg<'a, T>(args: &'a [Value], index: usize, label: &str) -> VmResult<T>
    where
        T: BorrowVmValue<'a>,
    {
        let value = args
            .get(index)
            .ok_or_else(|| VmError::HostError(format!("missing argument: {label}")))?;
        T::borrow_vm_value(value, label)
    }

    impl BorrowVmValue<'_> for i64 {
        fn borrow_vm_value(value: &Value, _label: &str) -> VmResult<Self> {
            match value {
                Value::Int(value) => Ok(*value),
                _ => Err(VmError::TypeMismatch("int")),
            }
        }
    }

    trait IntoVmValue {
        fn into_vm_value(self) -> Value;
    }

    impl IntoVmValue for bool {
        fn into_vm_value(self) -> Value {
            Value::Bool(self)
        }
    }

    impl IntoVmValue for i64 {
        fn into_vm_value(self) -> Value {
            Value::Int(self)
        }
    }

    fn return_one<T: IntoVmValue>(value: VmResult<T>) -> VmResult<CallOutcome> {
        Ok(CallOutcome::Return(CallReturn::one(value?.into_vm_value())))
    }

    pub(super) mod bevy {
        use super::*;

        /// Calls Bevy World::contains_entity for the current entity.
        #[pd_host_function(name = "bevy::World::contains_entity")]
        pub(crate) fn world_contains_entity_impl() -> VmResult<bool> {
            with_world(|world, entity| Ok(world.get_entity(entity).is_ok()))
        }

        pub(crate) fn world_contains_entity_host(args: &[Value]) -> VmResult<CallOutcome> {
            return_one(world_contains_entity(args))
        }

        /// Reads Health via Bevy World::get for the current entity.
        #[pd_host_function(name = "bevy::World::get_health")]
        pub(crate) fn world_get_health_impl() -> VmResult<i64> {
            with_world(|world, entity| {
                world
                    .get::<Health>(entity)
                    .map(|value| value.0)
                    .ok_or_else(|| {
                        VmError::HostError(format!("entity {entity:?} is missing Health"))
                    })
            })
        }

        pub(crate) fn world_get_health_host(args: &[Value]) -> VmResult<CallOutcome> {
            return_one(world_get_health(args))
        }

        /// Reads Armor via Bevy World::get for the current entity.
        #[pd_host_function(name = "bevy::World::get_armor")]
        pub(crate) fn world_get_armor_impl() -> VmResult<i64> {
            with_world(|world, entity| {
                world
                    .get::<Armor>(entity)
                    .map(|value| value.0)
                    .ok_or_else(|| {
                        VmError::HostError(format!("entity {entity:?} is missing Armor"))
                    })
            })
        }

        pub(crate) fn world_get_armor_host(args: &[Value]) -> VmResult<CallOutcome> {
            return_one(world_get_armor(args))
        }

        /// Writes Health via Bevy World::get_mut for the current entity.
        #[pd_host_function(name = "bevy::World::set_health")]
        pub(crate) fn world_set_health_impl(value: i64) -> VmResult<bool> {
            with_world(|world, entity| {
                let mut health = world.get_mut::<Health>(entity).ok_or_else(|| {
                    VmError::HostError(format!("entity {entity:?} is missing Health"))
                })?;
                health.0 = value;
                Ok(true)
            })
        }

        pub(crate) fn world_set_health_host(args: &[Value]) -> VmResult<CallOutcome> {
            return_one(world_set_health(args))
        }
    }
}

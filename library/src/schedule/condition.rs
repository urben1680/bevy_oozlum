use std::{
    any::TypeId,
    borrow::Cow,
    mem::replace,
    sync::atomic::{AtomicU32, Ordering},
};

use bevy::{
    ecs::{
        archetype::ArchetypeComponentId,
        component::{ComponentId, Tick},
        query::Access,
        schedule::{
            Condition, InternedSystemSet, IntoSystemSetConfigs, SystemSet, SystemSetConfigs,
        },
        system::{IntoSystem, ReadOnlySystem, System, SystemIn},
        world::{unsafe_world_cell::UnsafeWorldCell, DeferredWorld, World},
    },
    utils::default,
};

use crate::{
    error_per_flag,
    log::{OutOfLog, SparseTransitionLog},
    meta::{RevDirection, RevMeta},
    schedule::ForwardSet,
};

pub(crate) fn add_condition<Marker>(
    configs: &mut SystemSetConfigs,
    condition: impl Condition<Marker>,
) -> InternedSystemSet {
    #[derive(SystemSet, Copy, Clone, Debug, Hash, PartialEq, Eq)]
    struct ConditionSet(u32);
    static SET_COUNTER: AtomicU32 = AtomicU32::new(0);

    let condition = RevCondition {
        condition: IntoSystem::into_system(condition),
        meta_id: None,
        log: default(),
        component_access: default(),
        archetype_component_access: default(),
        out_of_log_err: false,
    };
    let set = ConditionSet(SET_COUNTER.fetch_add(1, Ordering::Relaxed)).intern();
    let before = replace(configs, ForwardSet.into_configs());
    *configs = (before, set.run_if(condition)).into_configs();
    set
}

struct RevCondition<T> {
    condition: T,
    meta_id: Option<ComponentId>,
    log: SparseTransitionLog<()>,
    component_access: Access<ComponentId>,
    archetype_component_access: Access<ArchetypeComponentId>,
    out_of_log_err: bool,
}

impl<T: ReadOnlySystem<Out = bool>> RevCondition<T> {
    fn run_inner(
        &mut self,
        meta: &RevMeta,
        valid: bool,
        eval_cond: impl FnOnce(&mut T) -> bool,
    ) -> bool {
        match meta.direction() {
            RevDirection::NOT_LOG => {
                let out = valid && eval_cond(&mut self.condition);
                self.log.push_and_pop_past(
                    meta.past_world_states().saturating_sub(1) as usize,
                    out.then_some(())
                );
                out
            },
            // todo, simplify error msg, can only be internal bug
            RevDirection::FORWARD_LOG => {
                match self.log.forward_log() {
                    Ok(option) => option.is_some(),
                    Err(OutOfLog) => error_per_flag!(&mut self.out_of_log_err, "Reversible condition {} got out of log. \
                        Make sure the reversible schedule this condition is in is correctly called in both the forward and backward direction. \
                        It seems that one or more backward schedule calls were missed. \
                        If this condition is in the RevUpdate schedule, this is likely a crate bug.\n{meta:?}", self.name())
                }
            },
            RevDirection::BackwardLog => {
                match self.log.backward_log() {
                    Ok(option) => option.is_some(),
                    Err(OutOfLog) => error_per_flag!(&mut self.out_of_log_err, "Reversible condition {} got out of log. \
                        Make sure the reversible schedule this condition is in is correctly called in both the forward and backward direction. \
                        It seems that one or more forward schedule calls were missed. \
                        If this condition is in the RevUpdate schedule, this is likely a crate bug.\n{meta:?}", self.name())
                }
            }
        }
    }
}

impl<T: ReadOnlySystem<Out = bool>> System for RevCondition<T> {
    type In = T::In;
    type Out = bool;
    fn name(&self) -> Cow<'static, str> {
        self.condition.name()
    }
    fn type_id(&self) -> TypeId {
        self.condition.type_id()
    }
    fn component_access(&self) -> &Access<ComponentId> {
        &self.component_access
    }
    fn archetype_component_access(&self) -> &Access<ArchetypeComponentId> {
        &self.archetype_component_access
    }
    fn is_send(&self) -> bool {
        self.condition.is_send()
    }
    fn is_exclusive(&self) -> bool {
        self.condition.is_exclusive()
    }
    fn has_deferred(&self) -> bool {
        self.condition.has_deferred()
    }
    fn initialize(&mut self, world: &mut World) {
        self.condition.initialize(world);
        self.meta_id = Some(world.register_resource::<RevMeta>());
        self.component_access
            .extend(self.condition.component_access());
        self.archetype_component_access
            .extend(self.condition.archetype_component_access());
        RevMeta::add_read_if_no_write(
            world,
            &mut self.component_access,
            &mut self.archetype_component_access,
        );
    }
    unsafe fn validate_param_unsafe(&mut self, world: UnsafeWorldCell) -> bool {
        unsafe {
            // SAFETY:
            // - Registered read access to resource
            // - T cannot have write access because T: ReadOnlySystem
            world.get_resource_by_id(self.meta_id.unwrap())
        }
        .map(|ptr| {
            // SAFETY:
            // todo
            ptr.deref::<RevMeta>()
        })
        .map(RevMeta::get_direction)
        .is_some()
    }
    fn validate_param(&mut self, world: &World) -> bool {
        world
            .get_resource_by_id(self.meta_id.unwrap())
            .map(|ptr| unsafe {
                // SAFETY:
                // todo
                ptr.deref::<RevMeta>()
            })
            .map(RevMeta::get_direction)
            .is_some()
    }
    unsafe fn run_unsafe(&mut self, input: SystemIn<'_, Self>, world: UnsafeWorldCell) -> bool {
        let meta = unsafe {
            // SAFETY:
            // - Registered read access to resource
            // - T cannot have write access because T: ReadOnlySystem
            world.get_resource_by_id(self.meta_id.unwrap())
        }
        .expect("Self::validate_param ensured Some");
        // SAFETY:
        // todo
        let meta = meta.deref::<RevMeta>();
        // SAFETY:
        // todo
        let valid = self.condition.validate_param_unsafe(world);
        self.run_inner(meta, valid, |cond| unsafe {
            // SAFETY:
            // - condition registered it's own accesses that were used to update Self's accesses
            // - update_archetype_component_access was called by the caller of Self::run_unsafe
            cond.run_unsafe(input, world)
        })
    }
    fn run(&mut self, input: SystemIn<'_, Self>, world: &mut World) -> Self::Out {
        let meta = {
            world.get_resource::<RevMeta>() // todo: by_id
        }
        .expect("Self::validate_param ensured Some")
        .clone();
        let valid = self.condition.validate_param(world);
        self.run_inner(&meta, valid, |cond| cond.run(input, world))
    }
    fn apply_deferred(&mut self, _world: &mut World) {}
    fn queue_deferred(&mut self, _world: DeferredWorld) {}
    fn default_system_sets(&self) -> Vec<InternedSystemSet> {
        self.condition.default_system_sets()
    }
    fn update_archetype_component_access(&mut self, world: UnsafeWorldCell) {
        self.condition.update_archetype_component_access(world);
        self.archetype_component_access
            .extend(self.condition.archetype_component_access());
    }
    fn check_change_tick(&mut self, change_tick: Tick) {
        self.condition.check_change_tick(change_tick);
    }
    fn get_last_run(&self) -> Tick {
        self.condition.get_last_run()
    }
    fn set_last_run(&mut self, last_run: Tick) {
        self.condition.set_last_run(last_run);
    }
}

// SAFETY:
// todo
unsafe impl<T: ReadOnlySystem<Out = bool>> ReadOnlySystem for RevCondition<T> {}

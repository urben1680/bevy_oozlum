use std::{
    any::TypeId,
    borrow::Cow,
    marker::PhantomData,
    sync::{Arc, Mutex},
};

use bevy::ecs::{
    archetype::ArchetypeComponentId,
    component::{ComponentId, Tick},
    query::Access,
    schedule::{Condition, InternedSystemSet},
    system::{IntoSystem, ReadOnlySystem, System},
    world::World,
    world::{unsafe_world_cell::UnsafeWorldCell, DeferredWorld},
};

use crate::{
    check_tick, error_per_flag,
    log::{OutOfLog, RareTransitionLog},
    meta::{Direction, RevMeta},
};

struct RevConditionForward<T> {
    condition: T,
    log: Arc<Mutex<RareTransitionLog<()>>>,
    name: String,
    component_access: Access<ComponentId>,
    archetype_component_access: Access<ArchetypeComponentId>,
    rev_meta_err: bool,
    lock_err: bool,
    out_of_log_err: bool,
    direction_err: bool,
}

struct RevConditionBackward<In: Send + Sync + 'static> {
    sets: Vec<InternedSystemSet>,
    log: Arc<Mutex<RareTransitionLog<()>>>,
    name: String,
    component_access: Access<ComponentId>,
    archetype_component_access: Access<ArchetypeComponentId>,
    tick: Tick,
    rev_meta_err: bool,
    lock_err: bool,
    out_of_log_err: bool,
    direction_err: bool,
    _in: PhantomData<fn(In)>,
}

pub(crate) fn forward_backward_conditions<
    In: Send + Sync + 'static,
    Marker,
    T: IntoSystem<In, bool, Marker, System: ReadOnlySystem>,
>(
    condition: T,
) -> (impl Condition<(), In>, impl Condition<(), In>) {
    let condition: T::System = IntoSystem::into_system(condition);
    let sets = condition.default_system_sets();

    let mut name_forward = condition.name().into_owned();
    let mut name_backward = name_forward.clone();
    name_forward.push_str(" (forward condition)");
    name_backward.push_str(" (backward condition)");

    let log: Arc<Mutex<RareTransitionLog<()>>> = Default::default();

    let forward = RevConditionForward {
        condition,
        log: log.clone(),
        name: name_forward,
        component_access: Default::default(),
        archetype_component_access: Default::default(),
        rev_meta_err: false,
        lock_err: false,
        out_of_log_err: false,
        direction_err: false,
    };

    let backward = RevConditionBackward {
        sets,
        log,
        name: name_backward,
        component_access: Default::default(),
        archetype_component_access: Default::default(),
        tick: Tick::new(0), // set by initialize
        rev_meta_err: false,
        lock_err: false,
        out_of_log_err: false,
        direction_err: false,
        _in: PhantomData,
    };

    (forward, backward)
}

impl<T: ReadOnlySystem<Out = bool>> System for RevConditionForward<T> {
    type In = T::In;
    type Out = bool;
    fn name(&self) -> Cow<'static, str> {
        Cow::Owned(self.name.clone())
    }
    fn type_id(&self) -> TypeId {
        TypeId::of::<Self>()
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
    unsafe fn run_unsafe(&mut self, input: Self::In, world: UnsafeWorldCell) -> Self::Out {
        let meta = unsafe {
            // SAFETY:
            // - Registered read access to resource
            // - T cannot have write access because T: ReadOnlySystem
            world.get_resource::<RevMeta>()
        };
        self.run_inner(meta, |cond| unsafe {
            // SAFETY:
            // - condition registered it's own accesses that were used to update Self's accesses
            // - update_archetype_component_access was called by the caller of Self::run_unsafe
            cond.run_unsafe(input, world)
        })
    }
    fn run(&mut self, input: Self::In, world: &mut World) -> Self::Out {
        self.run_readonly(input, world)
    }
    fn apply_deferred(&mut self, world: &mut World) {
        self.condition.apply_deferred(world)
    }
    fn queue_deferred(&mut self, world: DeferredWorld) {
        self.condition.queue_deferred(world)
    }
    fn initialize(&mut self, world: &mut World) {
        self.condition.initialize(world);
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
    fn update_archetype_component_access(&mut self, world: UnsafeWorldCell) {
        self.condition.update_archetype_component_access(world);
        self.archetype_component_access
            .extend(self.condition.archetype_component_access());
    }
    fn check_change_tick(&mut self, change_tick: Tick) {
        self.condition.check_change_tick(change_tick)
    }
    fn default_system_sets(&self) -> Vec<InternedSystemSet> {
        self.condition.default_system_sets()
    }
    fn get_last_run(&self) -> Tick {
        self.condition.get_last_run()
    }
    fn set_last_run(&mut self, last_run: Tick) {
        self.condition.set_last_run(last_run)
    }
}

// SAFETY: Self::run does ot mutate world
unsafe impl<T: ReadOnlySystem<Out = bool>> ReadOnlySystem for RevConditionForward<T> {
    fn run_readonly(&mut self, input: Self::In, world: &World) -> Self::Out {
        let meta = world.get_resource::<RevMeta>();
        self.run_inner(meta, |cond| cond.run_readonly(input, world))
    }
}

impl<In: Send + Sync + 'static> System for RevConditionBackward<In> {
    type In = In;
    type Out = bool;
    fn name(&self) -> Cow<'static, str> {
        Cow::Owned(self.name.clone())
    }
    fn type_id(&self) -> TypeId {
        TypeId::of::<Self>()
    }
    fn component_access(&self) -> &Access<ComponentId> {
        &self.component_access
    }
    fn archetype_component_access(&self) -> &Access<ArchetypeComponentId> {
        &self.archetype_component_access
    }
    fn is_send(&self) -> bool {
        true
    }
    fn is_exclusive(&self) -> bool {
        false
    }
    fn has_deferred(&self) -> bool {
        false
    }
    unsafe fn run_unsafe(&mut self, _input: Self::In, world: UnsafeWorldCell) -> Self::Out {
        let meta = unsafe {
            // SAFETY: Registered read access to resource
            world.get_resource::<RevMeta>()
        };
        self.run_inner(meta)
    }
    fn run(&mut self, _input: Self::In, world: &mut World) -> Self::Out {
        let meta = world.get_resource::<RevMeta>();
        self.run_inner(meta)
    }
    fn apply_deferred(&mut self, _world: &mut World) {}
    fn queue_deferred(&mut self, _world: DeferredWorld) {}
    fn initialize(&mut self, world: &mut World) {
        RevMeta::add_read_if_no_write(
            world,
            &mut self.component_access,
            &mut self.archetype_component_access,
        );
        self.tick = world.change_tick();
    }
    fn update_archetype_component_access(&mut self, _world: UnsafeWorldCell) {}
    fn check_change_tick(&mut self, change_tick: Tick) {
        check_tick(&mut self.tick, change_tick);
    }
    fn default_system_sets(&self) -> Vec<InternedSystemSet> {
        self.sets.clone()
    }
    fn get_last_run(&self) -> Tick {
        self.tick
    }
    fn set_last_run(&mut self, last_run: Tick) {
        self.tick = last_run;
    }
}

unsafe impl<In: Send + Sync + 'static> ReadOnlySystem for RevConditionBackward<In> {
    fn run_readonly(&mut self, _input: Self::In, world: &World) -> Self::Out {
        let meta = world.get_resource::<RevMeta>();
        self.run_inner(meta)
    }
}

impl<T: ReadOnlySystem<Out = bool>> RevConditionForward<T> {
    fn run_inner(
        &mut self,
        meta: Option<&RevMeta>,
        eval_cond: impl FnOnce(&mut T) -> bool,
    ) -> bool {
        let Some(meta) = meta else {
            return error_per_flag!(
                &mut self.rev_meta_err,
                "Reversible condition {} could not find RevMeta resource.",
                self.name,
            );
        };
        let mut log = match self.log.try_lock() {
            Ok(log) => log,
            Err(err) => {
                return error_per_flag!(
                    &mut self.lock_err,
                    "Reversible condition {} could not acquire internal log ({err:?}). This is likely a crate bug.\n{meta:?}",
                    self.name
                )
            }
        };
        match meta.get_direction() {
            Some(Direction::Forward) => {
                let out = eval_cond(&mut self.condition);
                log.pop_past_by_len(meta.past_len().saturating_sub(1));
                log.push_present(out.then_some(()));
                out
            }
            Some(Direction::ForwardLog) => match log.forward_log() {
                Ok(option) => option.is_some(),
                Err(OutOfLog) => error_per_flag!(&mut self.out_of_log_err, "Reversible condition {} got out of log. \
                    Make sure the reversible schedule this condition is in is correctly called in both the forward and backward direction. \
                    It seems that one or more backward schedule calls were missed. \
                    If this condition is in the RevUpdate schedule, this is likely a crate bug.\n{meta:?}", self.name)
            }
            _ => error_per_flag!(&mut self.direction_err, "Reversible condition {} did run while RevMeta was in a wrong direction. \
                If RevMeta was not manually overwritten during a reversible schedule, this is likely a crate bug.\n{meta:?}", self.name),
        }
    }
}

impl<In: Send + Sync + 'static> RevConditionBackward<In> {
    fn run_inner(&mut self, meta: Option<&RevMeta>) -> bool {
        let Some(meta) = meta else {
            return error_per_flag!(
                &mut self.rev_meta_err,
                "Reversible condition {} could not find RevMeta resource.",
                self.name
            );
        };
        if meta.get_direction() != Some(Direction::BackwardLog) {
            return error_per_flag!(
                &mut self.direction_err,
                "Reversible condition {} did run while RevMeta was in a wrong direction. \
                If RevMeta was not manually overwritten during a reversible schedule, this is likely a crate bug.\n{meta:?}",
                self.name
            );
        }
        let mut log = match self.log.try_lock() {
            Ok(log) => log,
            Err(err) => {
                return error_per_flag!(
                    &mut self.lock_err,
                    "Reversible condition {} could not acquire internal log ({err:?}). This is likely a crate bug.\n{meta:?}",
                    self.name
                )
            }
        };
        match log.backward_log() {
            Ok(option) => option.is_some(),
            Err(OutOfLog) => error_per_flag!(&mut self.out_of_log_err, "Reversible condition {} got out of log. \
                Make sure the reversible schedule this condition is in is correctly called in both the forward and backward direction. \
                It seems that one or more forward schedule calls were missed. \
                If this condition is in the RevUpdate schedule, this is likely a crate bug.\n{meta:?}", self.name)
        }
    }
}

use std::{
    any::TypeId,
    borrow::Cow,
    marker::PhantomData,
    sync::{Arc, Mutex},
};

use bevy::{
    ecs::{
        archetype::ArchetypeComponentId,
        component::{ComponentId, Tick},
        query::Access,
        schedule::InternedSystemSet,
        world::{unsafe_world_cell::UnsafeWorldCell, DeferredWorld},
    },
    prelude::{Condition, IntoSystem, ReadOnlySystem, Res, System, SystemSet, World},
    utils::tracing::error
};

use crate::{
    app::{check_tick, EMPTY_ARCHETYPE_COMPONENT_ACCESS, EMPTY_COMPONENT_ACCESS},
    log::{OutOfLog, RareTransitionLog},
    meta::{Direction, RevMeta},
};

macro_rules! error_once {
    ($flag:expr, $($arg:tt)+) => ({
        if !$flag {
            error!($($arg)+);
            $flag = true;
        }
        false
    })
}

struct RevConditionForward<T> {
    condition: T,
    log: Arc<Mutex<RareTransitionLog<()>>>,
    name: String,
    component_access: Access<ComponentId>,
    archetype_component_access: Access<ArchetypeComponentId>,
    rev_meta_err: bool,
    lock_err: bool,
    out_of_log_err: bool,
    direction_err: bool
}

struct RevConditionBackward<In: Send + Sync + 'static> {
    log: Arc<Mutex<RareTransitionLog<()>>>,
    name: String,
    tick: Tick,
    component_access: Access<ComponentId>,
    archetype_component_access: Access<ArchetypeComponentId>,
    _in: PhantomData<fn(In)>,
}

#[derive(SystemSet, Copy, Clone, Debug, Hash, PartialEq, Eq)]
struct ConditionSet;

pub(crate) fn forward_backward_conditions<
    In: Send + Sync + 'static,
    Marker,
    T: IntoSystem<In, bool, Marker, System: ReadOnlySystem>,
>(
    condition: T,
) -> (impl Condition<(), In>, impl Condition<(), In>) {
    let condition: T::System = IntoSystem::into_system(condition);

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
        direction_err: false
    };

    let backward = RevConditionBackward {
        log,
        name: name_backward,
        tick: Tick::new(0),
        component_access: Default::default(),
        archetype_component_access: Default::default(),
        _in: PhantomData,
    };

    (forward, backward)
}

impl<T: System<Out = bool> + ReadOnlySystem> System for RevConditionForward<T> {
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
            // SAFETY: Registered read access to resource
            world.get_resource::<RevMeta>()
        };
        // no clone needed because it is expected that condition as a ReadOnlySystem would not mutate RevMeta
        let Some(meta) = meta else {
            return error_once!(self.rev_meta_err, "Reversible condition {} (forward) could not find RevMeta resource.", self.name);
        };
        let Ok(mut log) = self.log.try_lock() else {
            return error_once!(self.lock_err, "Reversible condition {} (forward) could not lock internal log. \
                This is likely an internal bug.", self.name);
        };
        match meta.get_direction() {
            Some(Direction::Forward) => {
                let out = unsafe {
                    // SAFETY:
                    // - condition registered it's own accesses that were used to update Self's accesses
                    // - update_archetype_component_access was called by the caller of Self::run_unsafe
                    self.condition.run_unsafe(input, world)
                };
                log.pop_past_by_len(meta.past_len());
                log.push_present(out.then_some(().into()));
                out
            }
            Some(Direction::ForwardLog) => match log.forward_log() {
                Ok(option) => option.is_some(),
                Err(OutOfLog) => error_once!(self.out_of_log_err, "Reversible condition {} (forward) got out of log. \
                Make sure the reversible schedule this condition is in is correctly called in both the forward and backward direction. \
                It seems that one or more backward schedule calls were missed. \
                If this condition is in the RevUpdate schedule, this is likely an internal bug.", self.name)
            }
            _ => error_once!(self.direction_err, "Reversible condition {} (forward) did run while RevMeta was in a wrong direction ({:?}).\
                If RevMeta was not manually overridden this is likely an internal bug.", self.name, meta.internal_direction()),
        }
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
        let mut reads_meta = IntoSystem::into_system(|_: Res<RevMeta>| {});

        self.condition.initialize(world);
        reads_meta.initialize(world);

        self.component_access.extend(reads_meta.component_access());
        self.component_access
            .extend(self.condition.component_access());

        self.archetype_component_access
            .extend(reads_meta.archetype_component_access());
        self.archetype_component_access
            .extend(self.condition.archetype_component_access());
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
        vec![ConditionSet.intern()]
    }
    fn get_last_run(&self) -> Tick {
        self.condition.get_last_run()
    }
    fn set_last_run(&mut self, last_run: Tick) {
        self.condition.set_last_run(last_run)
    }
}

// SAFETY: todo
unsafe impl<T: System<Out = bool> + ReadOnlySystem> ReadOnlySystem for RevConditionForward<T> {
    fn run_readonly(&mut self, input: Self::In, world: &World) -> Self::Out {
        let Some(meta) = world.get_resource::<RevMeta>() else {
            todo!()
        };
        let mut log = self.log.try_lock().unwrap();
        match meta.get_direction() {
            Some(Direction::Forward) => {
                let out = self.condition.run_readonly(input, world);
                log.pop_past_by_len(meta.past_len());
                log.push_present(out.then_some(().into()));
                out
            }
            Some(Direction::ForwardLog) => log.forward_log().expect("todo").is_some(),
            _ => todo!(),
        }
    }
}

impl<In: Send + Sync + 'static> RevConditionBackward<In> {
    fn run_inner(&mut self, meta: Option<&RevMeta>) -> bool {
        if !matches!(
            meta.and_then(RevMeta::get_direction),
            Some(Direction::BackwardLog)
        ) {
            todo!()
        }
        self.log
            .try_lock()
            .unwrap()
            .backward_log()
            .expect("todo")
            .is_some()
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
        // todo add RevMeta read access
        &EMPTY_COMPONENT_ACCESS
    }
    fn archetype_component_access(&self) -> &Access<ArchetypeComponentId> {
        // todo add RevMeta read access
        &EMPTY_ARCHETYPE_COMPONENT_ACCESS
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
        self.run_inner(world.get_resource::<RevMeta>())
    }
    fn apply_deferred(&mut self, _world: &mut World) {}
    fn queue_deferred(&mut self, _world: DeferredWorld) {}
    fn initialize(&mut self, world: &mut World) {
        self.tick = world.change_tick();
        let mut reads_meta = IntoSystem::into_system(|_: Res<RevMeta>| {});
        reads_meta.initialize(world);
        self.component_access.extend(reads_meta.component_access());
        self.archetype_component_access
            .extend(reads_meta.archetype_component_access());
    }
    fn update_archetype_component_access(&mut self, _world: UnsafeWorldCell) {}
    fn check_change_tick(&mut self, change_tick: Tick) {
        check_tick(&mut self.tick, change_tick);
    }
    fn default_system_sets(&self) -> Vec<InternedSystemSet> {
        vec![ConditionSet.intern()]
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
        self.run_inner(world.get_resource::<RevMeta>())
    }
}
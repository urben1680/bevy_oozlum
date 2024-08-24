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
};

use crate::{
    app::{check_tick, EMPTY_ARCHETYPE_COMPONENT_ACCESS, EMPTY_COMPONENT_ACCESS},
    log::{OnePerFrame, RareTransitionLog},
    meta::{Direction, RevMeta},
};

struct RevConditionForward<T> {
    condition: T,
    log: Arc<Mutex<RareTransitionLog<OnePerFrame>>>,
    name: String,
    component_access: Access<ComponentId>,
    archetype_component_access: Access<ArchetypeComponentId>,
}

struct RevConditionBackward<In: Send + Sync + 'static> {
    log: Arc<Mutex<RareTransitionLog<OnePerFrame>>>,
    name: String,
    tick: Tick,
    _in: PhantomData<In>,
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

    let log: Arc<Mutex<RareTransitionLog<OnePerFrame>>> = Default::default();

    let forward = RevConditionForward {
        condition,
        log: log.clone(),
        name: name_forward,
        component_access: Default::default(),
        archetype_component_access: Default::default(),
    };

    let backward = RevConditionBackward {
        log,
        name: name_backward,
        tick: Tick::new(0),
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
        let meta = meta.expect(RevMeta::EXIST_MSG);
        let mut log = self.log.try_lock().unwrap();
        if meta.direction() == Direction::Forward {
            let out = unsafe {
                // SAFETY:
                // - condition registered it's own accesses that were used to update Self's accesses
                // - update_archetype_component_access was called by the caller of Self::run_unsafe
                self.condition.run_unsafe(input, world)
            };
            log.pop_past_by_len(meta);
            log.push_present(out.then_some(().into()));
            out
        } else {
            log.forward_log().expect("todo").is_some()
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
        let meta = world.get_resource::<RevMeta>().expect(RevMeta::EXIST_MSG);
        let mut log = self.log.try_lock().unwrap();
        if meta.direction() == Direction::Forward {
            let out = self.condition.run_readonly(input, world);
            log.pop_past_by_len(meta);
            log.push_present(out.then_some(().into()));
            out
        } else {
            log.forward_log().expect("todo").is_some()
        }
    }
}

impl<In: Send + Sync + 'static> RevConditionBackward<In> {
    fn run_inner(&mut self) -> bool {
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
        &EMPTY_COMPONENT_ACCESS
    }
    fn archetype_component_access(&self) -> &Access<ArchetypeComponentId> {
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
    unsafe fn run_unsafe(&mut self, _input: Self::In, _world: UnsafeWorldCell) -> Self::Out {
        self.run_inner()
    }
    fn run(&mut self, _input: Self::In, _world: &mut World) -> Self::Out {
        self.run_inner()
    }
    fn apply_deferred(&mut self, _world: &mut World) {}
    fn queue_deferred(&mut self, _world: DeferredWorld) {}
    fn initialize(&mut self, world: &mut World) {
        self.tick = world.change_tick();
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
    fn run_readonly(&mut self, _input: Self::In, _world: &World) -> Self::Out {
        self.run_inner()
    }
}

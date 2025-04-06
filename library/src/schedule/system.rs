use std::{
    any::{type_name, TypeId},
    borrow::Cow,
    fmt::Debug,
    sync::{Arc, Mutex, MutexGuard},
};

use bevy::{
    ecs::{
        archetype::ArchetypeComponentId,
        component::{ComponentId, Tick},
        query::Access,
        schedule::{ApplyDeferred, InternedSystemSet, IntoScheduleConfigs},
        system::{
            IntoSystem, ReadOnlySystem, ScheduleSystem, System, SystemIn, SystemInput,
            SystemParamValidationError,
        },
        world::{unsafe_world_cell::UnsafeWorldCell, DeferredWorld, World},
    },
    utils::default,
};

use crate::{
    meta::RevMeta,
    schedule::{
        error_per_flag, BackwardSet, BwdCmdSet, BwdCmdSysSet, BwdSysSet, ForwardSet, FwdSysSet,
    },
    undo_redo::{UndoRedoBuffer, UndoRedoLog},
};

use super::{AtomicSet, RevScheduleConfigs};

pub(super) fn rev_system<T, In, Out, M1, M2>(system: T) -> RevScheduleConfigs<ScheduleSystem>
where
    T: IntoSystem<In, Out, M1>,
    RevSystem<T::System, true>: IntoScheduleConfigs<ScheduleSystem, M2>,
    RevSystem<T::System, false>: IntoScheduleConfigs<ScheduleSystem, M2>,
    In: SystemInput,
{
    let system = IntoSystem::into_system(system);
    let sys_name = system.name();
    let unique_set = AtomicSet::new(sys_name.clone());

    if system.type_id() == TypeId::of::<ApplyDeferred>() {
        return RevScheduleConfigs::from_apply_deferred(
            ApplyDeferred.into_configs(),
            ApplyDeferred.in_set(unique_set),
            unique_set,
        );
    }

    let name = |string: &str| {
        let mut name = String::with_capacity(sys_name.len() + string.len());
        name.extend([&sys_name, string]);
        name
    };
    let forward_system_name = name(" (forward system)");
    let backward_commands_name = name(" (backward commands)");
    let backward_system_name = name(" (backward system)");

    let default_system_sets = system.default_system_sets();

    let inner = Mutex::new(Inner {
        system,
        initialized: false,
        commands_log: default(),
    });

    let shared = Arc::new(Shared {
        inner,
        default_system_sets: default_system_sets.clone(),
    });

    let forward_systems = RevSystem::<_, true> {
        shared: shared.clone(),
        name: forward_system_name,
        tick: default(),
        is_send: default(),
        is_exclusive: default(),
        has_deferred: default(),
        commands_err: false,
        component_access: default(),
        archetype_component_access: default(),
    }
    .in_set(unique_set)
    .in_set(FwdSysSet(unique_set))
    .in_set(ForwardSet);

    let backward_commands = CommandsBackward {
        shared: shared.clone(),
        name: backward_commands_name,
        tick: default(),
        has_deferred: default(),
        commands_err: false,
    }
    .in_set(unique_set)
    .in_set(BwdCmdSet(unique_set))
    .in_set(BwdCmdSysSet(unique_set))
    .in_set(BackwardSet);

    let backward_systems = RevSystem::<_, false> {
        shared,
        name: backward_system_name,
        tick: default(),
        is_send: default(),
        is_exclusive: default(),
        has_deferred: default(),
        commands_err: false,
        component_access: default(),
        archetype_component_access: default(),
    }
    .in_set(unique_set)
    .in_set(BwdSysSet(unique_set))
    .in_set(BwdCmdSysSet(unique_set))
    .in_set(BackwardSet)
    .after(BwdCmdSet(unique_set));

    let mut configs = RevScheduleConfigs {
        forward_systems,
        backward_commands,
        backward_systems,
        backward_commands_systems: BwdCmdSysSet(unique_set).into_configs(),
        conditioned: unique_set.into_configs(),
        conditions: Vec::new(),
    };

    for set in default_system_sets {
        configs.in_set_inner(set)
    }

    configs
}

pub(super) struct RevSystem<T, const FORWARD: bool> {
    shared: Arc<Shared<T>>,
    name: String,
    tick: Tick,
    commands_err: bool,
    is_send: bool,
    is_exclusive: bool,
    has_deferred: bool,

    // these need to be cloned because `&'a Access` cannot be returned from owned `MutexGuard<'a, Access>`
    component_access: Access<ComponentId>,
    archetype_component_access: Access<ArchetypeComponentId>,
}

struct Shared<T> {
    inner: Mutex<Inner<T>>,
    default_system_sets: Vec<InternedSystemSet>,
}

struct Inner<T> {
    system: T,
    initialized: bool,
    commands_log: UndoRedoLog,
}

impl<T, const FORWARD: bool> Debug for RevSystem<T, FORWARD> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(type_name::<Self>())
            .field("shared", &self.shared)
            .field("name", &self.name)
            .field("tick", &self.tick)
            .field("commands_err", &self.commands_err)
            .field("is_send", &self.is_send)
            .field("is_exclusive", &self.is_exclusive)
            .field("has_deferred", &self.has_deferred)
            .field("component_access", &self.component_access)
            .field(
                "archetype_component_access",
                &self.archetype_component_access,
            )
            .finish()
    }
}

impl<T> Debug for Shared<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(type_name::<Self>())
            .field("default_system_sets", &self.default_system_sets)
            .finish_non_exhaustive()
    }
}

impl<T: System, const FORWARD: bool> System for RevSystem<T, FORWARD> {
    type In = T::In;
    type Out = T::Out;

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
        self.is_send
    }
    fn is_exclusive(&self) -> bool {
        self.is_exclusive
    }
    fn has_deferred(&self) -> bool {
        self.has_deferred
    }
    unsafe fn validate_param_unsafe(
        &mut self,
        world: UnsafeWorldCell,
    ) -> Result<(), SystemParamValidationError> {
        // commands log needs to read RevMeta
        if self.is_exclusive && world.get_resource::<RevMeta>().is_none() {
            return Err(SystemParamValidationError::invalid());
        }
        self.shared
            .inner
            .try_lock()
            .unwrap_or_else(expect_lock(&self.name))
            .system
            .validate_param_unsafe(world)
    }
    fn validate_param(&mut self, world: &World) -> Result<(), SystemParamValidationError> {
        // commands log needs to read RevMeta
        if self.is_exclusive && !world.contains_resource::<RevMeta>() {
            return Err(SystemParamValidationError::invalid());
        }
        self.shared
            .inner
            .try_lock()
            .unwrap_or_else(expect_lock(&self.name))
            .system
            .validate_param(world)
    }
    unsafe fn run_unsafe(
        &mut self,
        input: SystemIn<'_, Self>,
        world: UnsafeWorldCell,
    ) -> Self::Out {
        let mut shared = self
            .shared
            .inner
            .try_lock()
            .unwrap_or_else(expect_lock(&self.name));

        if !self.is_exclusive() {
            shared.system.run_unsafe(input, world)
        } else if FORWARD {
            let out = shared.system.run_unsafe(input, world);
            // SAFETY: exclusive systems have full unique access to the world
            if let Err(err) = shared.commands_log.forward(world.world_mut(), &self.name) {
                error_per_flag!(
                    &mut self.commands_err,
                    "Reversible commands from reversible exclusive system {} could not be done/redone: {err:#?}",
                    self.name
                )
            }
            out
        } else {
            // SAFETY: exclusive systems have full unique access to the world
            if let Err(err) = shared.commands_log.backward(world.world_mut(), &self.name) {
                error_per_flag!(
                    &mut self.commands_err,
                    "Reversible commands from reversible exclusive system {} could not be undone: {err:#?}",
                    self.name
                )
            }
            shared.system.run_unsafe(input, world)
        }
    }
    fn apply_deferred(&mut self, world: &mut World) {
        if self.is_exclusive {
            // ExclusiveSystemFunction::apply_deferred is noop
            return;
        }

        let mut shared = self
            .shared
            .inner
            .try_lock()
            .unwrap_or_else(expect_lock(&self.name));

        shared.system.apply_deferred(world);

        if !FORWARD {
            return;
        }

        // reverisble commands are now in the buffer resource so commands_log can take them
        if let Err(err) = shared.commands_log.forward(world, &self.name) {
            error_per_flag!(&mut self.commands_err, "{err}")
        }
    }
    fn queue_deferred(&mut self, _world: DeferredWorld) {
        unimplemented!("{} used as an observer", std::any::type_name::<T>())
    }
    fn initialize(&mut self, world: &mut World) {
        let shared = initialize_inner(&mut self.shared, &mut self.tick, &self.name, world);
        self.is_send = shared.system.is_send();
        self.is_exclusive = shared.system.is_exclusive();
        self.has_deferred = shared.system.has_deferred();
        self.component_access
            .extend(shared.system.component_access());
        self.archetype_component_access
            .extend(shared.system.archetype_component_access());
    }
    fn update_archetype_component_access(&mut self, world: UnsafeWorldCell) {
        // reference: CombinatorSystem
        let system = &mut self
            .shared
            .inner
            .try_lock()
            .unwrap_or_else(expect_lock(&self.name))
            .system;
        system.update_archetype_component_access(world);
        self.archetype_component_access
            .extend(system.archetype_component_access());
    }
    fn check_change_tick(&mut self, change_tick: Tick) {
        check_tick(&mut self.tick, change_tick);
    }
    fn default_system_sets(&self) -> Vec<InternedSystemSet> {
        self.shared.default_system_sets.clone()
    }
    fn get_last_run(&self) -> Tick {
        self.tick
    }
    fn set_last_run(&mut self, last_run: Tick) {
        self.tick = last_run;
    }
}

// SAFETY: todo
unsafe impl<T: ReadOnlySystem<In = (), Out = ()>, const FORWARD: bool> ReadOnlySystem
    for RevSystem<T, FORWARD>
{
    fn run_readonly(&mut self, input: SystemIn<'_, Self>, world: &World) -> Self::Out {
        self.shared
            .inner
            .try_lock()
            .unwrap_or_else(expect_lock(&self.name))
            .system
            .run_readonly(input, world)
    }
}

struct CommandsBackward<T> {
    shared: Arc<Shared<T>>,
    name: String,
    tick: Tick,
    has_deferred: bool,
    commands_err: bool,
}

impl<T: System> System for CommandsBackward<T> {
    type In = ();
    type Out = ();
    fn name(&self) -> Cow<'static, str> {
        Cow::Owned(self.name.clone())
    }
    fn component_access(&self) -> &Access<ComponentId> {
        static EMPTY: Access<ComponentId> = Access::new();
        &EMPTY
    }
    fn archetype_component_access(&self) -> &Access<ArchetypeComponentId> {
        static EMPTY: Access<ArchetypeComponentId> = Access::new();
        &EMPTY
    }
    fn is_send(&self) -> bool {
        true
    }
    fn is_exclusive(&self) -> bool {
        false
    }
    fn has_deferred(&self) -> bool {
        self.has_deferred
    }
    unsafe fn validate_param_unsafe(
        &mut self,
        world: UnsafeWorldCell,
    ) -> Result<(), SystemParamValidationError> {
        // noop if has no deferred
        if !self.has_deferred {
            return Err(SystemParamValidationError::skipped());
        }
        // keep symmetry?
        self.shared
            .inner
            .try_lock()
            .unwrap_or_else(expect_lock(&self.name))
            .system
            .validate_param_unsafe(world)
    }
    fn validate_param(&mut self, world: &World) -> Result<(), SystemParamValidationError> {
        // noop if has no deferred
        if !self.has_deferred {
            return Err(SystemParamValidationError::skipped());
        }
        // keep symmetry?
        self.shared
            .inner
            .try_lock()
            .unwrap_or_else(expect_lock(&self.name))
            .system
            .validate_param(world)
    }
    unsafe fn run_unsafe(&mut self, _input: (), _world: UnsafeWorldCell) {}
    fn apply_deferred(&mut self, world: &mut World) {
        let result = self
            .shared
            .inner
            .try_lock()
            .unwrap_or_else(expect_lock(&self.name))
            .commands_log
            .backward(world, &self.name);
        if let Err(err) = result {
            error_per_flag!(&mut self.commands_err, "{err}")
        }
    }
    fn queue_deferred(&mut self, _world: DeferredWorld) {
        unreachable!("{} used as an observer", std::any::type_name::<T>())
    }
    fn check_change_tick(&mut self, change_tick: Tick) {
        check_tick(&mut self.tick, change_tick);
    }
    fn get_last_run(&self) -> Tick {
        self.tick
    }
    fn set_last_run(&mut self, last_run: Tick) {
        self.tick = last_run;
    }
    fn default_system_sets(&self) -> Vec<InternedSystemSet> {
        self.shared.default_system_sets.clone()
    }
    fn initialize(&mut self, world: &mut World) {
        let shared = initialize_inner(&mut self.shared, &mut self.tick, &self.name, world);
        self.has_deferred = shared.system.has_deferred();
    }
    fn update_archetype_component_access(&mut self, _world: UnsafeWorldCell) {}
}

// SAFETY: noop run_readonly
unsafe impl<T: System> ReadOnlySystem for CommandsBackward<T> {
    fn run_readonly(&mut self, _input: (), _world: &World) {}
}

fn initialize_inner<'a, T: System>(
    shared: &'a mut Arc<Shared<T>>,
    tick: &mut Tick,
    name: &String,
    world: &mut World,
) -> MutexGuard<'a, Inner<T>> {
    world.init_resource::<UndoRedoBuffer>();
    *tick = world.change_tick();
    let mut shared = shared.inner.try_lock().unwrap_or_else(expect_lock(name));
    if !shared.initialized {
        shared.system.initialize(world);
        shared.initialized = true;
    }
    shared
}

fn expect_lock<T: Debug, Out>(name: &String) -> impl FnOnce(T) -> Out + '_ {
    move |err| panic!("Could not access reversible system {name} because of {err:#?}")
}

/// reference: Tick::check_tick
fn check_tick(this: &mut Tick, change_tick: Tick) {
    let age = change_tick.get().wrapping_sub(this.get());
    if age > Tick::MAX.get() {
        *this = Tick::new(change_tick.get().wrapping_sub(Tick::MAX.get()));
    }
}

#[cfg(test)]
mod test {
    use bevy::{
        app::{App, Update},
        ecs::{
            change_detection::ResMut,
            component::{Component, HookContext},
            event::Event,
            observer::Trigger,
            schedule::IntoScheduleConfigs,
            system::{Commands, IntoSystem},
            world::{DeferredWorld, World},
        },
    };

    use crate::{prelude::*, schedule::test::panic_on_error_events};

    fn blank_undo_redo(_: &mut World, _: UndoRedoDirection) {}

    #[derive(Event)]
    struct Observer;
    fn observer(_: Trigger<Observer>, mut world: DeferredWorld) {
        world.buffer_undo_redo(blank_undo_redo);
        world.commands().queue(|world: &mut World| {
            world.spawn(EmptyOnAdd);
        });
    }

    #[derive(Event)]
    struct EmptyObserver;
    fn empty_observer(_: Trigger<Observer>, mut world: DeferredWorld) {
        world.buffer_undo_redo(blank_undo_redo);
    }

    #[derive(Component)]
    #[component(on_add = on_add)]
    struct OnAdd;
    fn on_add(mut world: DeferredWorld, _: HookContext) {
        world.buffer_undo_redo(blank_undo_redo);
        world.commands().queue(|world: &mut World| {
            world.trigger(EmptyObserver);
        });
    }

    #[derive(Component)]
    #[component(on_add = empty_on_add)]
    struct EmptyOnAdd;
    fn empty_on_add(mut world: DeferredWorld, _: HookContext) {
        world.buffer_undo_redo(blank_undo_redo);
    }

    fn assert_system_drains_all_undo_redo<M>(system: impl IntoSystem<(), (), M> + Copy + 'static) {
        panic_on_error_events();
        let mut app = App::new();
        app.add_plugins(RevSystemsPlugin::add_meta_and_runner(
            RevMeta::default(),
            Update,
        ))
        // non-reversible systems should leak undo_redo into the next reversible system
        .add_systems(RevUpdate, system.before(RevSystemsSet))
        .rev_add_systems(RevUpdate, system)
        .add_observer(observer)
        .add_observer(empty_observer)
        .update();
        let buffer = app.world().resource::<UndoRedoBuffer>();
        assert!(buffer.is_empty(), "{buffer:?}");
    }

    #[test]
    fn non_exclusive_system_drains_all_undo_redo() {
        assert_system_drains_all_undo_redo(
            |mut buffer: ResMut<UndoRedoBuffer>, mut commands: Commands| {
                buffer.buffer_undo_redo(blank_undo_redo);
                commands.buffer_undo_redo(blank_undo_redo);
                commands.queue(|world: &mut World| {
                    world.trigger(Observer);
                    world.spawn(OnAdd);
                });
            },
        )
    }

    #[test]
    fn exclusive_system_drains_all_undo_redo() {
        assert_system_drains_all_undo_redo(|world: &mut World| {
            world.buffer_undo_redo(blank_undo_redo);
            world.trigger(Observer);
            world.spawn(OnAdd);
        })
    }
}

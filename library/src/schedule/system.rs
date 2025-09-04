use std::{
    any::{TypeId, type_name},
    fmt::Debug,
    hash::Hash,
    sync::{
        Arc, Mutex, MutexGuard,
        atomic::{AtomicU32, Ordering},
    },
};

use bevy::{
    ecs::{
        component::{CheckChangeTicks, ComponentId, Tick},
        query::FilteredAccessSet,
        schedule::{ApplyDeferred, InternedSystemSet, IntoScheduleConfigs, SystemSet},
        system::{
            IntoSystem, ReadOnlySystem, RunSystemError, ScheduleSystem, System, SystemIn,
            SystemInput, SystemParamValidationError, SystemStateFlags,
        },
        world::{DeferredWorld, World, unsafe_world_cell::UnsafeWorldCell},
    },
    log::warn,
    utils::{default, prelude::DebugName},
};

use crate::{
    meta::RevMeta,
    schedule::{
        BackwardSystems, BwdCmdSet, BwdCmdSysSet, BwdSysSet, ForwardSystems, FwdSysSet,
        error_per_flag,
    },
    undo_redo::{UndoRedoBuffer, UndoRedoLog},
};

use super::RevScheduleConfigs;

pub(super) fn into_rev_system<T, In, Out, M1, M2>(system: T) -> RevScheduleConfigs<ScheduleSystem>
where
    T: IntoSystem<In, Out, M1>,
    In: SystemInput,
    RevSystem<T::System, true>: IntoScheduleConfigs<ScheduleSystem, M2>,
    RevSystem<T::System, false>: IntoScheduleConfigs<ScheduleSystem, M2>,
{
    let system = IntoSystem::into_system(system);
    let sys_name = system.name();
    let unified = AtomicSet::new(sys_name.clone()).intern();

    if system.type_id() == TypeId::of::<ApplyDeferred>() {
        return RevScheduleConfigs {
            forward_systems: ApplyDeferred.into_configs(),
            backward_commands: ApplyDeferred.in_set(unified),
            backward_systems: ApplyDeferred.in_set(unified),
            backward_commands_systems: unified.into_configs(),
            unified: unified.into_configs(),
        };
    }

    let name = |string: &str| {
        let sys_name = sys_name.as_string();
        let mut name = String::with_capacity(sys_name.len() + string.len());
        name.extend([&sys_name, string]);
        DebugName::owned(name)
    };
    let forward_system_name = name(" (forward system)");
    let backward_commands_name = name(" (backward commands)");
    let backward_system_name = name(" (backward system)");

    let default_system_sets = system.default_system_sets();

    let inner = Mutex::new(Inner {
        system,
        access: None,
        commands_log: default(),
    });

    let shared = Arc::new(Shared {
        inner,
        default_system_sets: default_system_sets.clone(),
    });

    let forward_systems = RevSystem::<_, true>::new(shared.clone(), forward_system_name)
        .in_set(unified)
        .in_set(FwdSysSet(unified))
        .in_set(ForwardSystems);

    let backward_commands = CommandsBackward::new(shared.clone(), backward_commands_name)
        .in_set(unified)
        .in_set(BwdCmdSet(unified))
        .in_set(BwdCmdSysSet(unified))
        .in_set(BackwardSystems);

    let backward_systems = RevSystem::<_, false>::new(shared, backward_system_name)
        .in_set(unified)
        .in_set(BwdSysSet(unified))
        .in_set(BwdCmdSysSet(unified))
        .in_set(BackwardSystems)
        .after(BwdCmdSet(unified));

    let mut configs = RevScheduleConfigs {
        forward_systems,
        backward_commands,
        backward_systems,
        backward_commands_systems: BwdCmdSysSet(unified).into_configs(),
        unified: unified.into_configs(),
    };

    for set in default_system_sets {
        configs.rev_in_set_inner(set)
    }

    configs
}

#[derive(SystemSet, Clone, Debug, Eq)]
struct AtomicSet {
    id: u32,
    #[allow(dead_code)]
    name: DebugName,
}

impl PartialEq for AtomicSet {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

impl Hash for AtomicSet {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.id.hash(state)
    }
}

impl AtomicSet {
    fn new(name: DebugName) -> Self {
        static ID: AtomicU32 = AtomicU32::new(0);
        let id = ID.fetch_add(1, Ordering::Relaxed);
        if id == u32::MAX {
            warn!(
                "an internal atomic counter to create reversible systems is exhausted, \
                creating more may lead to multiple systems sharing the same run condition"
            );
        }
        Self { id, name }
    }
}

pub(super) struct RevSystem<T, const FORWARD: bool> {
    shared: Arc<Shared<T>>,
    name: DebugName,
    tick: Tick,
    flags: SystemStateFlags,
    commands_err: bool,
}

impl<T, const FORWARD: bool> RevSystem<T, FORWARD> {
    fn new(shared: Arc<Shared<T>>, name: DebugName) -> Self {
        Self {
            shared,
            name,
            tick: default(),
            flags: SystemStateFlags::empty(),
            commands_err: false,
        }
    }
}

struct Shared<T> {
    inner: Mutex<Inner<T>>,
    default_system_sets: Vec<InternedSystemSet>,
}

struct Inner<T> {
    system: T,
    commands_log: UndoRedoLog,
    access: Option<Box<AccessCache>>,
}

struct AccessCache {
    access: FilteredAccessSet<ComponentId>,
    last_access: bool,
}

impl<T, const FORWARD: bool> Debug for RevSystem<T, FORWARD> {
    // todo: remove?
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(type_name::<Self>())
            .field("shared", &self.shared)
            .field("name", &self.name)
            .field("tick", &self.tick)
            .field("commands_err", &self.commands_err)
            .finish_non_exhaustive()
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

    fn name(&self) -> DebugName {
        self.name.clone()
    }
    fn type_id(&self) -> TypeId {
        TypeId::of::<Self>()
    }
    fn flags(&self) -> SystemStateFlags {
        self.flags
    }
    unsafe fn validate_param_unsafe(
        &mut self,
        world: UnsafeWorldCell,
    ) -> Result<(), SystemParamValidationError> {
        // commands log needs to read RevMeta
        if self.is_exclusive() {
            let meta = unsafe {
                // SAFETY: todo
                world.get_resource::<RevMeta>()
            };
            if meta.is_none() {
                return Err(SystemParamValidationError::invalid::<Self>(
                    RevMeta::EXPECT_IN_WORLD,
                ));
            }
        }
        let system = &mut self
            .shared
            .inner
            .try_lock()
            .unwrap_or_else(expect_lock(&self.name))
            .system;
        unsafe {
            // SAFETY: todo
            system.validate_param_unsafe(world)
        }
    }
    fn validate_param(&mut self, world: &World) -> Result<(), SystemParamValidationError> {
        // commands log needs to read RevMeta
        if self.is_exclusive() && !world.contains_resource::<RevMeta>() {
            return Err(SystemParamValidationError::invalid::<Self>(
                RevMeta::EXPECT_IN_WORLD,
            ));
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
    ) -> Result<Self::Out, RunSystemError> {
        let mut shared = self
            .shared
            .inner
            .try_lock()
            .unwrap_or_else(expect_lock(&self.name));

        if !self.is_exclusive() {
            unsafe {
                // SAFETY: todo
                shared.system.run_unsafe(input, world)
            }
        } else if FORWARD {
            let out = unsafe {
                // SAFETY: todo
                shared.system.run_unsafe(input, world)
            };
            let world = unsafe {
                // SAFETY: exclusive systems have full unique access to the world
                world.world_mut()
            };
            if let Err(err) = shared.commands_log.forward(world, &self.name) {
                error_per_flag!(
                    &mut self.commands_err,
                    "Reversible commands from reversible exclusive system {} could not be done/redone: {err:#?}",
                    self.name
                )
            }
            out
        } else {
            {
                let world = unsafe {
                    // SAFETY: exclusive systems have full unique access to the world
                    world.world_mut()
                };
                if let Err(err) = shared.commands_log.backward(world, &self.name) {
                    error_per_flag!(
                        &mut self.commands_err,
                        "Reversible commands from reversible exclusive system {} could not be undone: {err:#?}",
                        self.name
                    )
                }
            }
            unsafe {
                // SAFETY: todo
                shared.system.run_unsafe(input, world)
            }
        }
    }
    fn apply_deferred(&mut self, world: &mut World) {
        if self.is_exclusive() {
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
    fn initialize(&mut self, world: &mut World) -> FilteredAccessSet<ComponentId> {
        let (shared, access) =
            initialize_inner(&mut self.shared, &mut self.tick, &self.name, world);
        self.flags = shared.system.flags();
        access
    }
    fn check_change_tick(&mut self, check: CheckChangeTicks) {
        self.tick.check_tick(check);
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
    fn run_readonly(
        &mut self,
        input: SystemIn<'_, Self>,
        world: &World,
    ) -> Result<Self::Out, RunSystemError> {
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
    name: DebugName,
    tick: Tick,
    has_deferred: bool,
    commands_err: bool,
}

impl<T> CommandsBackward<T> {
    fn new(shared: Arc<Shared<T>>, name: DebugName) -> Self {
        Self {
            shared,
            name,
            tick: default(),
            has_deferred: default(),
            commands_err: false,
        }
    }
}

impl<T: System> System for CommandsBackward<T> {
    type In = ();
    type Out = ();
    fn name(&self) -> DebugName {
        self.name.clone()
    }
    fn flags(&self) -> SystemStateFlags {
        if self.has_deferred {
            SystemStateFlags::DEFERRED
        } else {
            SystemStateFlags::empty()
        }
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
            return Err(SystemParamValidationError::skipped::<Self>(
                "system has no deferred",
            ));
        }
        // keep symmetry?
        let system = &mut self
            .shared
            .inner
            .try_lock()
            .unwrap_or_else(expect_lock(&self.name))
            .system;
        unsafe {
            // SAFETY: todo
            system.validate_param_unsafe(world)
        }
    }
    fn validate_param(&mut self, world: &World) -> Result<(), SystemParamValidationError> {
        // noop if has no deferred
        if !self.has_deferred {
            return Err(SystemParamValidationError::skipped::<Self>(
                "system has no deferred",
            ));
        }
        // keep symmetry?
        self.shared
            .inner
            .try_lock()
            .unwrap_or_else(expect_lock(&self.name))
            .system
            .validate_param(world)
    }
    unsafe fn run_unsafe(
        &mut self,
        _input: (),
        _world: UnsafeWorldCell,
    ) -> Result<(), RunSystemError> {
        Ok(())
    }
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
    fn check_change_tick(&mut self, check: CheckChangeTicks) {
        self.tick.check_tick(check);
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
    fn initialize(&mut self, world: &mut World) -> FilteredAccessSet<ComponentId> {
        let (shared, access) =
            initialize_inner(&mut self.shared, &mut self.tick, &self.name, world);
        self.has_deferred = shared.system.has_deferred();
        access
    }
}

// SAFETY: noop run_readonly
unsafe impl<T: System> ReadOnlySystem for CommandsBackward<T> {
    fn run_readonly(&mut self, _input: (), _world: &World) -> Result<(), RunSystemError> {
        Ok(())
    }
}

fn initialize_inner<'a, T: System>(
    shared: &'a mut Arc<Shared<T>>,
    tick: &mut Tick,
    name: &DebugName,
    world: &mut World,
) -> (MutexGuard<'a, Inner<T>>, FilteredAccessSet<ComponentId>) {
    world.init_resource::<UndoRedoBuffer>();
    *tick = world.change_tick();
    let mut shared = shared.inner.try_lock().unwrap_or_else(expect_lock(name));
    match shared.access.take() {
        None => {
            let access = shared.system.initialize(world);
            shared.access = Some(Box::new(AccessCache {
                access: access.clone(),
                last_access: false,
            }));
            (shared, access)
        }
        Some(mut access_cache) => {
            if access_cache.last_access {
                (shared, access_cache.access)
            } else {
                access_cache.last_access = true;
                let access = access_cache.access.clone();
                shared.access = Some(access_cache);
                (shared, access)
            }
        }
    }
}

fn expect_lock<T: Debug, Out>(name: &DebugName) -> impl FnOnce(T) -> Out + '_ {
    move |err| panic!("Could not access reversible system {name} because of {err:#?}")
}

#[cfg(test)]
mod test {
    use bevy::{
        app::{App, Update},
        ecs::{
            change_detection::{Res, ResMut},
            component::Component,
            event::Event,
            lifecycle::HookContext,
            observer::On,
            schedule::IntoScheduleConfigs,
            system::{Commands, IntoSystem},
            world::{DeferredWorld, World},
        },
    };

    use crate::{panic_on_error_events, prelude::*, undo_redo::UndoRedoBuffer};

    fn blank_undo_redo(_: &mut World, _: UndoRedoDirection) {}

    #[derive(Event)]
    struct Observer;

    fn observer(_: On<Observer>, mut world: DeferredWorld) {
        let now = world.resource::<RevMeta>().non_log_now().unwrap();
        world.buffer_undo_redo(now, blank_undo_redo);
        world.commands().queue(|world: &mut World| {
            world.spawn(EmptyOnAdd);
        });
    }

    #[derive(Event)]
    struct EmptyObserver;
    fn empty_observer(_: On<Observer>, mut world: DeferredWorld) {
        let now = world.resource::<RevMeta>().non_log_now().unwrap();
        world.buffer_undo_redo(now, blank_undo_redo);
    }

    #[derive(Component)]
    #[component(on_add = on_add)]
    struct OnAdd;
    fn on_add(mut world: DeferredWorld, _: HookContext) {
        let now = world.resource::<RevMeta>().non_log_now().unwrap();
        world.buffer_undo_redo(now, blank_undo_redo);
        world.commands().queue(|world: &mut World| {
            world.trigger(EmptyObserver);
        });
    }

    #[derive(Component)]
    #[component(on_add = empty_on_add)]
    struct EmptyOnAdd;
    fn empty_on_add(mut world: DeferredWorld, _: HookContext) {
        let now = world.resource::<RevMeta>().non_log_now().unwrap();
        world.buffer_undo_redo(now, blank_undo_redo);
    }

    fn assert_system_drains_all_undo_redo<M>(system: impl IntoSystem<(), (), M> + Copy + 'static) {
        panic_on_error_events();
        let mut app = App::new();
        app.add_plugins(RevPlugin::add_meta_and_runner(
            RevMeta::DEFAULT_MAX_WORLD_STATES,
            RevMeta::DEFAULT_PAUSED,
            Update,
        ))
        // non-reversible systems should leak undo_redo into the next reversible system
        .add_systems(RevUpdate, system.before(RevSystems))
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
            |mut buffer: ResMut<UndoRedoBuffer>, meta: Res<RevMeta>, mut commands: Commands| {
                let now = meta.non_log_now().unwrap();
                buffer.buffer_undo_redo(now, blank_undo_redo);
                commands.buffer_undo_redo(now, blank_undo_redo);
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
            let now = world.resource::<RevMeta>().non_log_now().unwrap();
            world.buffer_undo_redo(now, blank_undo_redo);
            world.trigger(Observer);
            world.spawn(OnAdd);
        })
    }
}

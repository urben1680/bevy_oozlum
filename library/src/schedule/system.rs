use crate::{
    schedule::{
        BackwardDeferredAndSystemSet, BackwardDeferredSet, BackwardSystemSet, BackwardSystems,
        ForwardSystemSet, ForwardSystems,
    },
    undo_redo::{UndoRedoBuffer, UndoRedoLog},
};
use bevy_ecs::{
    component::{CheckChangeTicks, Tick},
    error::BevyError,
    query::FilteredAccessSet,
    schedule::{ApplyDeferred, InternedSystemSet, IntoScheduleConfigs, SystemSet},
    system::{
        IntoSystem, ReadOnlySystem, RunSystemError, ScheduleSystem, System, SystemIn, SystemInput,
        SystemParamValidationError, SystemStateFlags,
    },
    world::{DeferredWorld, World, unsafe_world_cell::UnsafeWorldCell},
};
use bevy_log::{error, warn};
use bevy_utils::prelude::DebugName;
use std::sync::{
    Arc, Mutex, MutexGuard, TryLockError,
    atomic::{AtomicU32, Ordering},
};
use std::{
    any::{TypeId, type_name},
    fmt::Debug,
    hash::Hash,
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
            backward_deferred: ApplyDeferred.in_set(unified),
            backward_systems: ApplyDeferred.in_set(unified),
            backward_deferred_and_systems: unified.into_configs(),
            unified: unified.into_configs(),
        };
    }

    let name = |postfix: &str| {
        if size_of::<DebugName>() == 0 {
            return DebugName::borrowed("");
        }
        let sys_name: &str = sys_name.as_ref();
        DebugName::owned(format!("{sys_name}{postfix}"))
    };
    let forward_system_name = name(FORWARD_POSTFIX);
    let backward_deferred_name = name(DEFERRED_POSTFIX);
    let backward_system_name = name(BACKWARD_POSTFIX);

    let default_system_sets = system.default_system_sets();

    let inner = Mutex::new(Inner {
        system,
        access: None,
        deferred_log: Default::default(),
    });

    let shared = Arc::new(Shared {
        inner,
        default_system_sets: default_system_sets.clone(),
    });

    let forward_systems = RevSystem::<_, true>::new(shared.clone(), forward_system_name)
        .in_set(unified)
        .in_set(ForwardSystemSet(unified))
        .in_set(ForwardSystems);

    let backward_deferred = BackwardDeferred::new(shared.clone(), backward_deferred_name)
        .in_set(unified)
        .in_set(BackwardDeferredSet(unified))
        .in_set(BackwardDeferredAndSystemSet(unified))
        .in_set(BackwardSystems);

    let backward_systems = RevSystem::<_, false>::new(shared, backward_system_name)
        .in_set(unified)
        .in_set(BackwardSystemSet(unified))
        .in_set(BackwardDeferredAndSystemSet(unified))
        .in_set(BackwardSystems)
        .after(BackwardDeferredSet(unified));

    let mut configs = RevScheduleConfigs {
        forward_systems,
        backward_deferred,
        backward_systems,
        backward_deferred_and_systems: BackwardDeferredAndSystemSet(unified).into_configs(),
        unified: unified.into_configs(),
    };

    for set in default_system_sets {
        configs.rev_in_set_inner(set)
    }

    configs
}

const FORWARD_POSTFIX: &str = " (forward system)";
const DEFERRED_POSTFIX: &str = " (backward deferred)";
const BACKWARD_POSTFIX: &str = " (backward system)";

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

/// Is `pub(super)` for system set docs in parent module
pub(super) struct RevSystem<T, const FORWARD: bool> {
    shared: Arc<Shared<T>>,
    name: DebugName,
    tick: Tick,
    flags: SystemStateFlags,
    lock_or_deferred_err: bool,
}

impl<T, const FORWARD: bool> RevSystem<T, FORWARD> {
    fn new(shared: Arc<Shared<T>>, name: DebugName) -> Self {
        Self {
            shared,
            name,
            tick: Default::default(),
            flags: SystemStateFlags::empty(),
            lock_or_deferred_err: false,
        }
    }
    fn postfix() -> &'static str {
        if FORWARD {
            FORWARD_POSTFIX
        } else {
            BACKWARD_POSTFIX
        }
    }
}

struct Shared<T> {
    inner: Mutex<Inner<T>>,
    default_system_sets: Vec<InternedSystemSet>,
}

struct Inner<T> {
    system: T,
    deferred_log: UndoRedoLog,
    access: Option<Box<AccessCache>>,
}

struct AccessCache {
    access: FilteredAccessSet,
    last_access: bool,
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
        if self.is_exclusive() {
            // all exclusive system params are always available
            return Ok(());
        }
        let system = &mut self
            .shared
            .inner
            .try_lock()
            .map_err(try_lock_validation_err(&self.name, Self::postfix()))?
            .system;
        unsafe {
            // SAFETY: todo
            system.validate_param_unsafe(world)
        }
    }
    fn validate_param(&mut self, world: &World) -> Result<(), SystemParamValidationError> {
        if self.is_exclusive() {
            // all exclusive system params are always available
            return Ok(());
        }
        self.shared
            .inner
            .try_lock()
            .map_err(try_lock_validation_err(&self.name, Self::postfix()))?
            .system
            .validate_param(world)
    }
    unsafe fn run_unsafe(
        &mut self,
        input: SystemIn<'_, Self>,
        world: UnsafeWorldCell,
    ) -> Result<Self::Out, RunSystemError> {
        let mut shared = self.shared.inner.try_lock().map_err(try_lock_system_err)?;

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
            shared.deferred_log.forward(world)?;
            out
        } else {
            {
                let world = unsafe {
                    // SAFETY: exclusive systems have full unique access to the world
                    world.world_mut()
                };
                shared.deferred_log.backward(world)?;
            }
            unsafe {
                // SAFETY: todo
                shared.system.run_unsafe(input, world)
            }
        }
    }
    fn apply_deferred(&mut self, world: &mut World) {
        let mut result = || -> Result<(), BevyError> {
            let mut shared = self
                .shared
                .inner
                .try_lock()
                .map_err(|err| err.to_string())?;

            shared.system.apply_deferred(world);

            if !FORWARD {
                return Ok(());
            }

            // reverisble commands are now in the buffer resource so commands_log can take them
            shared.deferred_log.forward(world).map_err(Into::into)
        };

        // ExclusiveSystemFunction::apply_deferred is noop
        if !self.is_exclusive()
            && !self.lock_or_deferred_err
            && let Err(err) = result()
        {
            self.lock_or_deferred_err = true;
            if size_of::<DebugName>() == 0 {
                let name = type_name::<T>();
                error!(
                    "apply_deferred of reversible system {name}{} failed: {err}",
                    Self::postfix()
                )
            } else {
                error!(
                    "apply_deferred of reversible system {} failed: {err}",
                    self.name
                );
            };
        }
    }
    fn queue_deferred(&mut self, _world: DeferredWorld) {
        unreachable!("reversible systems are not used as observers")
    }
    fn initialize(&mut self, world: &mut World) -> FilteredAccessSet {
        let (shared, access) = initialize_inner(
            &mut self.shared,
            &mut self.tick,
            &self.name,
            Self::postfix(),
            world,
        );
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
            .map_err(try_lock_system_err)?
            .system
            .run_readonly(input, world)
    }
}

/// Is `pub(super)` for system set docs in parent module
pub(super) struct BackwardDeferred<T> {
    shared: Arc<Shared<T>>,
    name: DebugName,
    tick: Tick,
    has_deferred: bool,
    lock_or_deferred_err: bool,
}

impl<T> BackwardDeferred<T> {
    fn new(shared: Arc<Shared<T>>, name: DebugName) -> Self {
        Self {
            shared,
            name,
            tick: Default::default(),
            has_deferred: Default::default(),
            lock_or_deferred_err: false,
        }
    }
}

impl<T: System> System for BackwardDeferred<T> {
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
            return Err(SystemParamValidationError::skipped::<T>(
                "reversible system has no deferred parameters",
            ));
        }
        // keep symmetry?
        let system = &mut self
            .shared
            .inner
            .try_lock()
            .map_err(try_lock_validation_err(&self.name, DEFERRED_POSTFIX))?
            .system;
        unsafe {
            // SAFETY: todo
            system.validate_param_unsafe(world)
        }
    }
    fn validate_param(&mut self, world: &World) -> Result<(), SystemParamValidationError> {
        // noop if has no deferred
        if !self.has_deferred {
            return Err(SystemParamValidationError::skipped::<T>(
                "reversible system has no deferred parameters",
            ));
        }
        // keep symmetry?
        self.shared
            .inner
            .try_lock()
            .map_err(try_lock_validation_err(&self.name, DEFERRED_POSTFIX))?
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
        let mut result = || -> Result<(), BevyError> {
            self.shared
                .inner
                .try_lock()
                .map_err(|err| err.to_string())?
                .deferred_log
                .backward(world)
                .map_err(Into::into)
        };
        if !self.lock_or_deferred_err
            && let Err(err) = result()
        {
            self.lock_or_deferred_err = true;
            if size_of::<DebugName>() == 0 {
                let name = type_name::<T>();
                error!(
                    "deferred actions of reversible system {name}{DEFERRED_POSTFIX} could not be undone: {err}"
                )
            } else {
                error!(
                    "deferred actions of reversible system {} could not be undone: {err}",
                    self.name
                );
            };
        }
    }
    fn queue_deferred(&mut self, _world: DeferredWorld) {
        unreachable!("reversible systems are not used as observers")
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
    fn initialize(&mut self, world: &mut World) -> FilteredAccessSet {
        let (shared, access) = initialize_inner(
            &mut self.shared,
            &mut self.tick,
            &self.name,
            DEFERRED_POSTFIX,
            world,
        );
        self.has_deferred = shared.system.has_deferred();
        access
    }
}

// SAFETY: noop run_readonly
unsafe impl<T: System> ReadOnlySystem for BackwardDeferred<T> {
    fn run_readonly(&mut self, _input: (), _world: &World) -> Result<(), RunSystemError> {
        Ok(())
    }
}

fn initialize_inner<'a, T: System>(
    shared: &'a mut Arc<Shared<T>>,
    tick: &mut Tick,
    name: &DebugName,
    postfix: &'static str,
    world: &mut World,
) -> (MutexGuard<'a, Inner<T>>, FilteredAccessSet) {
    world.init_resource::<UndoRedoBuffer>();
    *tick = world.change_tick();
    let mut shared = shared.inner.try_lock().unwrap_or_else(|err| {
        if size_of::<DebugName>() == 0 {
            let name = type_name::<T>();
            panic!("reversible system {name}{postfix} could not be initialized: {err}")
        } else {
            panic!("reversible system {name} could not be initialized: {err}");
        };
    });
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

fn try_lock_system_err<T>(err: TryLockError<T>) -> RunSystemError {
    RunSystemError::Failed(err.to_string().into())
}

fn try_lock_validation_err<'a, T>(
    name: &'a DebugName,
    postfix: &'static str,
) -> impl for<'b> FnOnce(TryLockError<MutexGuard<'b, Inner<T>>>) -> SystemParamValidationError + 'a
{
    move |err| {
        if size_of::<DebugName>() == 0 {
            let name = type_name::<T>();
            SystemParamValidationError::invalid::<T>(format!(
                "param validation  of reversible system {name}{postfix} failed: {err}"
            ))
        } else {
            SystemParamValidationError::invalid::<T>(format!(
                "param validation of reversible system {name} failed: {err}"
            ))
        }
    }
}

#[cfg(test)]
mod test {
    use bevy_app::{App, Update};
    use bevy_ecs::{
        change_detection::{Res, ResMut},
        component::Component,
        event::Event,
        lifecycle::HookContext,
        observer::On,
        schedule::IntoScheduleConfigs,
        system::{Commands, IntoSystem},
        world::{DeferredWorld, World},
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

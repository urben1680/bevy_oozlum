use alloc::{format, string::ToString, vec::Vec};
use bevy_ecs::{
    change_detection::{CheckChangeTicks, Tick},
    error::BevyError,
    query::FilteredAccessSet,
    schedule::{ApplyDeferred, InternedSystemSet, IntoScheduleConfigs, SystemSet},
    system::{
        IntoSystem, RunSystemError, ScheduleSystem, System, SystemIn, SystemParamValidationError,
        SystemStateFlags,
    },
    world::{DeferredWorld, World, unsafe_world_cell::UnsafeWorldCell},
};
use bevy_log::error;
use bevy_platform::sync::{
    Arc, Mutex, MutexGuard, TryLockError,
    atomic::{AtomicU32, Ordering},
};
use bevy_utils::prelude::DebugName;
use core::{
    any::TypeId,
    fmt::Debug,
    hash::{Hash, Hasher},
};

use crate::{
    schedule::{
        BackwardDeferredAndSystemSet, BackwardDeferredSet, BackwardSystemSet, BackwardSystems,
        ForwardSystemSet, ForwardSystems,
    },
    undo_redo::UndoRedoLog,
};

use super::RevScheduleConfigs;

pub(super) fn into_rev_system<T, M1, M2>(system: T) -> RevScheduleConfigs<ScheduleSystem>
where
    T: IntoSystem<(), (), M1>, // parts of piping systems to not get converted, only as a whole
    RevSystem<T::System, true>: IntoScheduleConfigs<ScheduleSystem, M2>,
    RevSystem<T::System, false>: IntoScheduleConfigs<ScheduleSystem, M2>,
{
    let system = IntoSystem::into_system(system);

    if system.type_id() == TypeId::of::<ApplyDeferred>() {
        // ApplyDeferred has special handling at the scheduler so this is not wrapped in RevSystems
        return RevScheduleConfigs::from(ApplyDeferred);
    }

    if system.is_exclusive() {
        // Exclusive systems return true here as they do not need to be initialized first.
        // Allowing exclusive systems has the problem that using rev_* API before other,
        // non-buffering reversible system logic would make it impossible to ensure this order is
        // reversed when going backward. That can lead to bugs.
        // Instead, rev_* API should always come last. To enforce that, it makes more sense to use
        // DeferredWorld systems that explicitly use commands which make it obvious they are not
        // applied in the middle of the exclusive system.
        // This API limitation also is helpful in the context of UndoRedo implementations where
        // reversible operations, if there were any public ones intended for exclusive systems,
        // should not be used.
        // This limitation also makes the System impl for RevSystem more simple as reversible
        // exclusive systems are implemented differently and that does not need to be accounted for.
        unimplemented!(
            "exclusive systems are not supported to be reversible, use reversible commands instead"
        );
    }

    let name = system.name();
    // This set contains BackwardDeferred and both RevSystems of only this system instance. It is
    // the base for the other wrapping sets and for conditions to be used on.
    let unified = RevSystemTypeSet::new(name.clone()).intern();

    let name = |postfix: &str| DebugName::owned(format!("{name}{postfix}"));
    let forward_system_name = name(FORWARD_POSTFIX);
    let backward_deferred_name = name(DEFERRED_POSTFIX);
    let backward_system_name = name(BACKWARD_POSTFIX);

    let default_system_sets = system.default_system_sets();

    let shared = Arc::new(Shared {
        inner: Mutex::new(Inner::from(system)),
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
        conditioned: unified.into_configs(),
    };

    // all configs need to be in all default system sets so using T as a reference for ordering
    // works even when T consists of multiple systems in a pipe and this is ordered to one of such
    // systems and not T as a whole
    for set in default_system_sets {
        configs.rev_in_set_inner(set)
    }

    configs
}

const FORWARD_POSTFIX: &str = " (forward system)";
const DEFERRED_POSTFIX: &str = " (backward deferred)";
const BACKWARD_POSTFIX: &str = " (backward system)";

/// Reversible variant but no replacement of [`SystemTypeSet`](bevy_ecs::schedule::SystemTypeSet).
///
/// The only configuration will be reversible run conditions in [`RevScheduleConfigs::conditioned`]
/// where these sets are placed at.
// is `pub(super)` for docs in parent module
#[derive(SystemSet, Clone, Debug, Eq)]
pub(super) struct RevSystemTypeSet {
    id: u32,
    #[allow(dead_code)]
    name: DebugName,
}

impl PartialEq for RevSystemTypeSet {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

impl Hash for RevSystemTypeSet {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.id.hash(state)
    }
}

impl RevSystemTypeSet {
    fn new(name: DebugName) -> Self {
        static ID: AtomicU32 = AtomicU32::new(0);
        let id = ID.fetch_add(1, Ordering::Relaxed);
        if id == u32::MAX {
            // this technically is a warn and not an error, but detecting the actual first set after
            // overflow needs another atomic with stricter Ordering for both which is not worth it
            error!(
                "an internal atomic counter to create reversible systems is exhausted, \
                creating more may lead to multiple systems sharing the same run condition"
            );
        }
        Self { id, name }
    }
}

/// The system wrapper of which one instance with `FORWARD = true` is used in [`ForwardSystems`] and
/// one instance with `FORWARD = false` is used in [`BackwardSystems`].
// is `pub(super)` for docs in parent module
pub(super) struct RevSystem<T, const FORWARD: bool> {
    shared: Arc<Shared<T>>,
    name: DebugName,
    flags: SystemStateFlags,

    // todo: depcrecate with bevy 0.19, check run_unsafe err instead if needed
    skipped_with_deferred: bool,
}

impl<T, const FORWARD: bool> RevSystem<T, FORWARD> {
    fn new(shared: Arc<Shared<T>>, name: DebugName) -> Self {
        Self {
            shared,
            name,
            flags: SystemStateFlags::empty(),
            skipped_with_deferred: false,
        }
    }
}

struct Shared<T> {
    inner: Mutex<Inner<T>>,
    default_system_sets: Vec<InternedSystemSet>,
}

impl<T> Shared<T> {
    fn get_inner<'a>(&'a self, name: &DebugName) -> MutexGuard<'a, Inner<T>> {
        self.inner.try_lock().unwrap_or_else(|err| {
            panic!("reversible system {name} could not be accessed: {err}");
        })
    }
}

struct Inner<T> {
    system: T,
    deferred_log: UndoRedoLog,
    initialized: bool,
}

impl<T> From<T> for Inner<T> {
    fn from(system: T) -> Self {
        Self {
            system,
            deferred_log: Default::default(),
            initialized: false,
        }
    }
}

impl<T: System<In = (), Out = ()>, const FORWARD: bool> System for RevSystem<T, FORWARD> {
    type In = ();
    type Out = ();

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
        if FORWARD && self.skipped_with_deferred {
            return Err(SystemParamValidationError::invalid::<Self>(
                "a previous call of validate_param_unsafe that returned Ok(()) had to be \
                followed by a system run",
            ));
        }

        let system = &mut self
            .shared
            .inner
            .try_lock()
            .map_err(try_lock_validation_err(&self.name))?
            .system;
        // SAFETY: Self::initialize called T::initialize to register all access of T
        let result = unsafe { system.validate_param_unsafe(world) };

        debug_assert!(!FORWARD || !self.skipped_with_deferred);
        if FORWARD && self.has_deferred() && result.as_ref().is_err_and(|err| err.skipped) {
            self.skipped_with_deferred = true;
            return Ok(());
        }
        result
    }
    unsafe fn run_unsafe(
        &mut self,
        input: SystemIn<'_, Self>,
        world: UnsafeWorldCell,
    ) -> Result<(), RunSystemError> {
        if FORWARD && self.skipped_with_deferred {
            self.skipped_with_deferred = false;
            return Ok(());
        }
        let mut shared = self.shared.inner.try_lock().map_err(try_lock_system_err)?;
        // SAFETY: Self::initialize called T::initialize to register all access of T
        unsafe { shared.system.run_unsafe(input, world) }
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
                // `BackwardDeferred` is doing the backward log traversal
                return Ok(());
            }

            // reverisble commands are now in the buffer resource so commands_log can take them
            shared.deferred_log.forward(world).map_err(Into::into)
        };

        if let Err(err) = result() {
            error!(
                "apply_deferred of reversible system {} failed: {err}",
                self.name
            );
        }
    }
    fn queue_deferred(&mut self, _world: DeferredWorld) {
        unreachable!() // reversible systems are not used as observers
    }
    fn initialize(&mut self, world: &mut World) -> FilteredAccessSet {
        let mut inner = self.shared.get_inner(&self.name);
        let access = inner.system.initialize(world);
        inner.initialized = true;
        self.flags = inner.system.flags();
        access
    }
    fn check_change_tick(&mut self, check: CheckChangeTicks) {
        let mut inner = self.shared.get_inner(&self.name);
        inner.system.check_change_tick(check);
    }
    fn default_system_sets(&self) -> Vec<InternedSystemSet> {
        self.shared.default_system_sets.clone()
    }
    fn get_last_run(&self) -> Tick {
        let inner = self.shared.get_inner(&self.name);
        inner.system.get_last_run()
    }
    fn set_last_run(&mut self, last_run: Tick) {
        let mut inner = self.shared.get_inner(&self.name);
        inner.system.set_last_run(last_run);
    }
}

/// The system that only applies [`UndoRedo::undo`](crate::undo_redo::UndoRedo::undo) of deferred
/// actions from `T`. If `T` has no deferred parameters or is exclusive, this is a noop system.
// is `pub(super)` for docs in parent module
pub(super) struct BackwardDeferred<T> {
    shared: Arc<Shared<T>>,
    tick: Tick,
    name: DebugName,
    has_deferred: bool,
}

impl<T> BackwardDeferred<T> {
    fn new(shared: Arc<Shared<T>>, name: DebugName) -> Self {
        Self {
            shared,
            tick: Tick::new(u32::MAX),
            name,
            has_deferred: Default::default(),
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
        _world: UnsafeWorldCell,
    ) -> Result<(), SystemParamValidationError> {
        // noop if has no deferred
        if !self.has_deferred {
            return Err(SystemParamValidationError::skipped::<T>(
                "reversible system has no deferred parameters",
            ));
        }

        // If T skipped this frame, then this does not need to be detected and mirrored here as
        // UndoRedoLog keeps track itself if it needs to run without getting out of sync
        Ok(())
    }
    unsafe fn run_unsafe(
        &mut self,
        _input: (),
        world: UnsafeWorldCell,
    ) -> Result<(), RunSystemError> {
        self.tick = world.increment_change_tick();
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

        if let Err(err) = result() {
            error!(
                "deferred actions of reversible system {} could not be undone: {err}",
                self.name
            );
        }
    }
    fn queue_deferred(&mut self, _world: DeferredWorld) {
        unreachable!(); // reversible systems are not used as observers
    }
    fn initialize(&mut self, world: &mut World) -> FilteredAccessSet {
        let mut inner = self.shared.get_inner(&self.name);
        if !inner.initialized {
            inner.system.initialize(world);
        }
        self.has_deferred = inner.system.has_deferred();
        FilteredAccessSet::new()
    }
    fn default_system_sets(&self) -> Vec<InternedSystemSet> {
        self.shared.default_system_sets.clone()
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
}

fn try_lock_system_err<T>(err: TryLockError<T>) -> RunSystemError {
    RunSystemError::Failed(err.to_string().into())
}

fn try_lock_validation_err<'a, T>(
    name: &'a DebugName,
) -> impl for<'b> FnOnce(TryLockError<MutexGuard<'b, Inner<T>>>) -> SystemParamValidationError + 'a
{
    move |err| {
        SystemParamValidationError::invalid::<T>(format!(
            "param validation of reversible system {name} failed: {err}"
        ))
    }
}

#[cfg(test)]
mod test {
    use bevy_app::{App, Update};
    use bevy_ecs::{
        change_detection::Res,
        component::Component,
        event::Event,
        lifecycle::HookContext,
        observer::On,
        resource::Resource,
        schedule::IntoScheduleConfigs,
        system::{Command, Commands, RunSystemError, SystemParamValidationError},
        world::{DeferredWorld, World},
    };

    use crate::{panic_on_error_events, prelude::*, undo_redo::UndoRedoBuffer};

    fn blank_undo_redo(_: &mut World, _: UndoRedoDirection) {}

    #[derive(Event)]
    struct Observer;

    fn observer(_: On<Observer>, mut world: DeferredWorld) {
        let not_log = world.resource::<RevMeta>().not_log();
        world
            .commands()
            .buffer_undo_redo(not_log, blank_undo_redo)
            .spawn(EmptyOnAdd);
    }

    #[derive(Event)]
    struct EmptyObserver;
    fn empty_observer(_: On<Observer>, mut world: DeferredWorld) {
        let not_log = world.resource::<RevMeta>().not_log();
        world.commands().buffer_undo_redo(not_log, blank_undo_redo);
    }

    #[derive(Component)]
    #[component(on_add = on_add)]
    struct OnAdd;
    fn on_add(mut world: DeferredWorld, _: HookContext) {
        let not_log = world.resource::<RevMeta>().not_log();
        world
            .commands()
            .buffer_undo_redo(not_log, blank_undo_redo)
            .trigger(EmptyObserver);
    }

    #[derive(Component)]
    #[component(on_add = empty_on_add)]
    struct EmptyOnAdd;
    fn empty_on_add(mut world: DeferredWorld, _: HookContext) {
        let not_log = world.resource::<RevMeta>().not_log();
        world.commands().buffer_undo_redo(not_log, blank_undo_redo);
    }

    #[test]
    fn non_exclusive_system_drains_all_undo_redo() {
        fn system(meta: Res<RevMeta>, mut commands: Commands) {
            let not_log = meta.not_log();
            commands.buffer_undo_redo(not_log, blank_undo_redo);
            commands.queue(|world: &mut World| {
                world.trigger(Observer);
                world.spawn(OnAdd);
            });
        }

        panic_on_error_events();
        let mut app = App::new();
        app.add_plugins(RevPlugin.set_runner_in_schedule(Update))
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
    fn skipping_system_does_not_skip_redo() {
        #[derive(Resource, Default)]
        struct Counter(u8);

        fn system1(not_log: NotLog, mut commands: Commands) {
            commands.redo_and_buffer(not_log, |world: &mut World, _: UndoRedoDirection| {
                world.get_resource_or_init::<Counter>().0 += 1;
            });
        }

        fn system2(not_log: NotLog, commands: Commands) -> Result<(), RunSystemError> {
            system1(not_log, commands);
            Err(RunSystemError::Skipped(
                SystemParamValidationError::skipped::<()>(""),
            ))
        }

        fn system3(meta: Res<RevMeta>, commands: Commands) -> Result<(), RunSystemError> {
            if let Some(not_log) = meta.get_not_log() {
                system1(not_log, commands);
            }
            Err(RunSystemError::Skipped(
                SystemParamValidationError::skipped::<()>(""),
            ))
        }

        panic_on_error_events();
        let mut app = App::new();
        app.add_plugins(RevPlugin.set_runner_in_schedule(Update))
            .rev_add_systems(RevUpdate, (system1, system2, system3));

        app.update();
        assert_eq!(app.world().resource::<Counter>().0, 3);

        RevQueue::RunBackwardLog.apply(app.world_mut()).unwrap();
        app.update();
        assert_eq!(app.world().resource::<Counter>().0, 6);

        RevQueue::RunForwardLog.apply(app.world_mut()).unwrap();
        app.update();
        assert_eq!(app.world().resource::<Counter>().0, 9);
    }
}

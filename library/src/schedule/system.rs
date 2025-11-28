use crate::{
    schedule::{
        BackwardDeferredAndSystemSet, BackwardDeferredSet, BackwardSystemSet, BackwardSystems,
        ForwardSystemSet, ForwardSystems,
    },
    undo_redo::UndoRedoLog,
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
use bevy_log::error;
use bevy_utils::prelude::DebugName;
use core::{
    any::{TypeId, type_name},
    fmt::Debug,
    hash::{Hash, Hasher},
};
use std::sync::{
    Arc, Mutex, MutexGuard, TryLockError,
    atomic::{AtomicU32, Ordering},
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
    let name = system.name();

    if system.type_id() == TypeId::of::<ApplyDeferred>() {
        // ApplyDeferred has special handling at the scheduler so this is not wrapped in RevSystems
        return RevScheduleConfigs::from(ApplyDeferred);
    }

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
}

impl<T, const FORWARD: bool> RevSystem<T, FORWARD> {
    fn new(shared: Arc<Shared<T>>, name: DebugName) -> Self {
        Self {
            shared,
            name,
            flags: SystemStateFlags::empty(),
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

impl<T> Shared<T> {
    fn get_inner<'a>(
        &'a self,
        name: &DebugName,
        postfix: &'static str,
    ) -> MutexGuard<'a, Inner<T>> {
        self.inner.try_lock().unwrap_or_else(|err| {
            if size_of::<DebugName>() == 0 {
                let name = type_name::<T>();
                panic!("reversible system {name}{postfix} could not be accessed: {err}")
            } else {
                panic!("reversible system {name} could not be accessed: {err}");
            };
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
            // SAFETY: Self::initialize called T::initialize to register all access of T
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
                // SAFETY: Self::initialize called T::initialize to register all access of T
                shared.system.run_unsafe(input, world)
            }
        } else if FORWARD {
            let out = unsafe {
                // SAFETY: exclusive system has full unique access to the world
                shared.system.run_unsafe(input, world)
            };
            let world = unsafe {
                // SAFETY: exclusive system has full unique access to the world
                world.world_mut()
            };
            shared.deferred_log.forward(world)?;
            out
        } else {
            {
                let world = unsafe {
                    // SAFETY: exclusive system has full unique access to the world
                    world.world_mut()
                };
                shared.deferred_log.backward(world)?;
            }
            unsafe {
                // SAFETY: exclusive system has full unique access to the world
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
                // `BackwardDeferred` is doing the backward log traversal
                return Ok(());
            }

            // reverisble commands are now in the buffer resource so commands_log can take them
            shared.deferred_log.forward(world).map_err(Into::into)
        };

        // ExclusiveSystemFunction::apply_deferred is noop
        if !self.is_exclusive()
            && let Err(err) = result()
        {
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
        let mut inner = self.shared.get_inner(&self.name, Self::postfix());
        let access = inner.system.initialize(world);
        inner.initialized = true;
        self.flags = inner.system.flags();
        access
    }
    fn check_change_tick(&mut self, check: CheckChangeTicks) {
        let mut inner = self.shared.get_inner(&self.name, Self::postfix());
        inner.system.check_change_tick(check);
    }
    fn default_system_sets(&self) -> Vec<InternedSystemSet> {
        self.shared.default_system_sets.clone()
    }
    fn get_last_run(&self) -> Tick {
        let inner = self.shared.get_inner(&self.name, Self::postfix());
        inner.system.get_last_run()
    }
    fn set_last_run(&mut self, last_run: Tick) {
        let mut inner = self.shared.get_inner(&self.name, Self::postfix());
        inner.system.set_last_run(last_run);
    }
}

// SAFETY: Self has no additional access to the world besides in System::apply_deferred to T
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
    fn initialize(&mut self, world: &mut World) -> FilteredAccessSet {
        let mut inner = self.shared.get_inner(&self.name, DEFERRED_POSTFIX);
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

// SAFETY: Self does not access the world as it is noop
unsafe impl<T: System> ReadOnlySystem for BackwardDeferred<T> {
    fn run_readonly(&mut self, _input: (), _world: &World) -> Result<(), RunSystemError> {
        Ok(())
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
        let past_len = world.resource::<RevMeta>().meta_past_len();
        world.buffer_undo_redo(past_len, blank_undo_redo);
        world.commands().queue(|world: &mut World| {
            world.spawn(EmptyOnAdd);
        });
    }

    #[derive(Event)]
    struct EmptyObserver;
    fn empty_observer(_: On<Observer>, mut world: DeferredWorld) {
        let past_len = world.resource::<RevMeta>().meta_past_len();
        world.buffer_undo_redo(past_len, blank_undo_redo);
    }

    #[derive(Component)]
    #[component(on_add = on_add)]
    struct OnAdd;
    fn on_add(mut world: DeferredWorld, _: HookContext) {
        let past_len = world.resource::<RevMeta>().meta_past_len();
        world.buffer_undo_redo(past_len, blank_undo_redo);
        world.commands().queue(|world: &mut World| {
            world.trigger(EmptyObserver);
        });
    }

    #[derive(Component)]
    #[component(on_add = empty_on_add)]
    struct EmptyOnAdd;
    fn empty_on_add(mut world: DeferredWorld, _: HookContext) {
        let past_len = world.resource::<RevMeta>().meta_past_len();
        world.buffer_undo_redo(past_len, blank_undo_redo);
    }

    fn assert_system_drains_all_undo_redo<M>(system: impl IntoSystem<(), (), M> + Copy + 'static) {
        panic_on_error_events();
        let mut app = App::new();
        app.add_plugins(RevPlugin::add_meta_and_runner(
            RevMeta::DEFAULT_MAX_PAST_LEN,
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
                let past_len = meta.meta_past_len();
                buffer.buffer_undo_redo(past_len, blank_undo_redo);
                commands.buffer_undo_redo(past_len, blank_undo_redo);
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
            let past_len = world.resource::<RevMeta>().meta_past_len();
            world.buffer_undo_redo(past_len, blank_undo_redo);
            world.trigger(Observer);
            world.spawn(OnAdd);
        })
    }
}

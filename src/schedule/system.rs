use alloc::{borrow::Cow, format, string::ToString, vec::Vec};
use bevy_ecs::{
    change_detection::{CheckChangeTicks, Tick},
    error::{BevyError, ErrorContext},
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
    Arc, Mutex, MutexGuard,
    atomic::{AtomicU32, Ordering},
};
use bevy_utils::DebugName;
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

    if system.system_type() == TypeId::of::<ApplyDeferred>() {
        // ApplyDeferred has special handling at the scheduler so this is not wrapped in RevSystems
        return RevScheduleConfigs::from(ApplyDeferred);
    }

    let name = system.name();

    if system.is_exclusive() {
        // Exclusive systems are not supported because of the following reasons:
        //
        // 1. A hypothetical public RevWorld API would do the direct effect, the "doing", like
        //    inserting a component anytime inside the exclusive system. Just like RevCommands this
        //    would store their UndoRedo in a resource that at the next sync point is stored in the
        //    system state. This has the consequence that the "undoing" and "redoing" can only
        //    happen at sync points.
        //    However, reversible logic that happens inside the exclusive system directly, not using
        //    sync points, can not be reliably ordered to UndoRedo logic without putting the burden
        //    on the user.
        //    For example, an exclusive system could rev_spawn an entity via a RevWorld API (1),
        //    then do non-UndoRedo logic based on the current RevDirection (2), and third do another
        //    UndoRedo-generating logic via RevWorld (3).
        //    This would be the order of actions depending on RevDirection:
        //     NotLog:      do (1), do (2), do (3), all in the exclusive system, not in a sync point
        //     BackwardLog: undo (3), undo (1) in a preceding sync point, undo (2) in the system
        //     ForwardLog:  redo (2) in the system, redo (1), redo (3) in a following sync point
        //    As one can see, the order is wrong. The user would have to actively refrain from using
        //    such a RevWorld API. Not offering such an API would be not enough as nothing hinders
        //    the user from directly applying reversible commands in the system.
        //    The above issue gets worse when mixed with other systems with non-UndoRedo reversible
        //    logic that run after the exclusive system but before a next sync point.
        // 2. Supporting reversible exclusive systems makes the RevSystem implementation more
        //    complicated, error prone and even more dependent on implementation details of
        //    ExclusiveFunctionSystem that would need to be mirrored here additionally to the
        //    FunctionSystem implementation. The needed public RevWorld API adds much more code to
        //    test and maintain.
        // 3. A RevWorld API might need to be designed entirely differently to RevCommands and the
        //    relation to RevDirection matching inside the exclusive system. While this may partly
        //    solve the issues as pointed out at 1., it adds to this crate's learning curve.
        unimplemented!(
            "exclusive systems as {name:?} are not supported to be reversible, \
            use reversible commands via Commands::as_rev instead of &mut World"
        );
    }

    // This set contains BackwardDeferred and both RevSystems of only this system instance. It is
    // the base for the other wrapping sets and for conditions to be used on.
    let unified = RevSystemTypeSet::new(name.clone()).intern();

    let name = |postfix: &str| DebugName::owned(format!("{name}{postfix}"));
    let forward_system_name = name(FORWARD_POSTFIX);
    let backward_deferred_name = name(DEFERRED_POSTFIX);
    let backward_system_name = name(BACKWARD_POSTFIX);

    let default_system_sets = system.default_system_sets();

    let inner = Arc::new(Mutex::new(Inner::from(system)));

    let forward_systems = RevSystem::<_, true>::new(inner.clone(), forward_system_name)
        .in_set(unified)
        .in_set(ForwardSystemSet(unified))
        .in_set(ForwardSystems);

    let backward_deferred = BackwardDeferred::new(inner.clone(), backward_deferred_name)
        .in_set(unified)
        .in_set(BackwardDeferredSet(unified))
        .in_set(BackwardDeferredAndSystemSet(unified))
        .in_set(BackwardSystems);

    let backward_systems = RevSystem::<_, false>::new(inner, backward_system_name)
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
    // this fully replaces System::default_system_sets of the System impls in this module
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
    inner: Arc<Mutex<Inner<T>>>,
    name: DebugName,
    flags: SystemStateFlags,
}

impl<T, const FORWARD: bool> RevSystem<T, FORWARD> {
    fn new(inner: Arc<Mutex<Inner<T>>>, name: DebugName) -> Self {
        Self {
            inner,
            name,
            flags: SystemStateFlags::empty(),
        }
    }
}

fn get_inner<'a, T>(inner: &'a Mutex<Inner<T>>, name: &DebugName) -> MutexGuard<'a, Inner<T>> {
    inner.try_lock().unwrap_or_else(|err| {
        panic!("reversible system {name} could not be accessed: {err}");
    })
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
    fn system_type(&self) -> TypeId {
        TypeId::of::<Self>()
    }
    fn flags(&self) -> SystemStateFlags {
        self.flags
    }
    unsafe fn run_unsafe(
        &mut self,
        input: SystemIn<'_, Self>,
        world: UnsafeWorldCell,
    ) -> Result<(), RunSystemError> {
        let system = &mut self
            .inner
            .try_lock()
            .map_err(|err| {
                SystemParamValidationError::invalid::<T>(format!(
                    "param validation of reversible system {} failed: {err}",
                    self.name
                ))
            })?
            .system;

        // SAFETY: Self::initialize called T::initialize to register all access of T
        let result = unsafe { system.run_unsafe(input, world) };

        if FORWARD && self.has_deferred() && matches!(result, Err(RunSystemError::Skipped(_))) {
            // if this system is skipped during ForwardLog, there may be reversible commands to be
            // redone, but the scheduler will not call System::apply_deferred if this does not
            // return Ok
            return Ok(());
        }

        result
    }
    #[cfg(feature = "hotpatching")]
    fn refresh_hotpatch(&mut self) {
        match self.inner.try_lock() {
            Ok(mut inner) => inner.system.refresh_hotpatch(),
            Err(err) => error!("could not hotpatch system {}: {err}", self.name),
        }
    }
    fn apply_deferred(&mut self, world: &mut World) {
        let mut last_run = Tick::new(0);

        let mut result = || -> Result<(), BevyError> {
            let mut inner = self.inner.try_lock().map_err(|err| err.to_string())?;

            last_run = inner.system.get_last_run();

            inner.system.apply_deferred(world);

            if !FORWARD {
                // `BackwardDeferred` is doing the backward log traversal
                return Ok(());
            }

            // reverisble commands are now in the queue resource so commands_log can take them
            inner.deferred_log.forward(world).map_err(Into::into)
        };

        if let Err(err) = result() {
            world.fallback_error_handler()(
                BevyError::error(format!(
                    "apply_deferred of reversible system {} failed: {err}",
                    self.name
                )),
                ErrorContext::System {
                    name: self.name.clone(),
                    last_run,
                },
            );
        }
    }
    fn queue_deferred(&mut self, _world: DeferredWorld) {
        unreachable!() // reversible systems are not used as observers
    }
    fn initialize(&mut self, world: &mut World) -> FilteredAccessSet {
        let mut inner = get_inner(&self.inner, &self.name);
        let access = inner.system.initialize(world);
        inner.initialized = true;
        self.flags = inner.system.flags();
        access
    }
    fn check_change_tick(&mut self, check: CheckChangeTicks) {
        let mut inner = get_inner(&self.inner, &self.name);
        inner.system.check_change_tick(check);
    }
    fn default_system_sets(&self) -> Vec<InternedSystemSet> {
        Vec::new() // already specified via rev_in_set_inner at into_rev_system
    }
    fn get_last_run(&self) -> Tick {
        let inner = get_inner(&self.inner, &self.name);
        inner.system.get_last_run()
    }
    fn set_last_run(&mut self, last_run: Tick) {
        let mut inner = get_inner(&self.inner, &self.name);
        inner.system.set_last_run(last_run);
    }
}

/// The system that only applies [`UndoRedo::undo`](crate::undo_redo::UndoRedo::undo) of deferred
/// actions from `T`. If `T` has no deferred parameters or is exclusive, this is a noop system.
// is `pub(super)` for docs in parent module
pub(super) struct BackwardDeferred<T> {
    inner: Arc<Mutex<Inner<T>>>,
    tick: Tick,
    name: DebugName,
    has_deferred: bool,
}

impl<T> BackwardDeferred<T> {
    fn new(inner: Arc<Mutex<Inner<T>>>, name: DebugName) -> Self {
        Self {
            inner,
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
    unsafe fn run_unsafe(
        &mut self,
        _input: (),
        world: UnsafeWorldCell,
    ) -> Result<(), RunSystemError> {
        if !self.has_deferred {
            return Err(RunSystemError::Skipped(
                SystemParamValidationError::skipped::<T>(Cow::Borrowed(
                    "reversible system has no deferred parameters",
                )),
            ));
        }
        self.tick = world.increment_change_tick();
        Ok(())
    }
    #[cfg(feature = "hotpatching")]
    fn refresh_hotpatch(&mut self) {}
    fn apply_deferred(&mut self, world: &mut World) {
        let mut last_run = Tick::new(0);

        let mut result = || -> Result<(), BevyError> {
            let mut inner = self.inner.try_lock().map_err(|err| err.to_string())?;
            last_run = inner.system.get_last_run();
            inner.deferred_log.backward(world).map_err(Into::into)
        };

        if let Err(err) = result() {
            world.fallback_error_handler()(
                BevyError::error(format!(
                    "deferred actions of reversible system {} could not be undone: {err}",
                    self.name
                )),
                ErrorContext::System {
                    name: self.name.clone(),
                    last_run,
                },
            );
        }
    }
    fn queue_deferred(&mut self, _world: DeferredWorld) {
        unreachable!(); // reversible systems are not used as observers
    }
    fn initialize(&mut self, world: &mut World) -> FilteredAccessSet {
        let mut inner = get_inner(&self.inner, &self.name);
        if !inner.initialized {
            inner.system.initialize(world);
        }
        self.has_deferred = inner.system.has_deferred();
        FilteredAccessSet::new()
    }
    fn default_system_sets(&self) -> Vec<InternedSystemSet> {
        Vec::new() // already specified via rev_in_set_inner at into_rev_system
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

    use crate::{panic_on_warnings_or_errors, prelude::*, undo_redo::UndoRedoQueue};

    fn blank_undo_redo(_: &mut World, _: UndoRedoDirection) {}

    #[derive(Event)]
    struct Observer;

    fn observer(_: On<Observer>, mut world: DeferredWorld) {
        let not_log = world.resource::<RevMeta>().not_log();
        world
            .commands()
            .as_rev(not_log)
            .queue_undo_redo(blank_undo_redo)
            .mut_non_rev()
            .spawn(EmptyOnAdd);
    }

    #[derive(Event)]
    struct EmptyObserver;
    fn empty_observer(_: On<Observer>, mut world: DeferredWorld) {
        let not_log = world.resource::<RevMeta>().not_log();
        world
            .commands()
            .as_rev(not_log)
            .queue_undo_redo(blank_undo_redo);
    }

    #[derive(Component)]
    #[component(on_add = on_add)]
    struct OnAdd;
    fn on_add(mut world: DeferredWorld, _: HookContext) {
        let not_log = world.resource::<RevMeta>().not_log();
        world
            .commands()
            .as_rev(not_log)
            .queue_undo_redo(blank_undo_redo)
            .mut_non_rev()
            .trigger(EmptyObserver);
    }

    #[derive(Component)]
    #[component(on_add = empty_on_add)]
    struct EmptyOnAdd;
    fn empty_on_add(mut world: DeferredWorld, _: HookContext) {
        let not_log = world.resource::<RevMeta>().not_log();
        world
            .commands()
            .as_rev(not_log)
            .queue_undo_redo(blank_undo_redo);
    }

    #[test]
    fn non_exclusive_system_drains_all_undo_redo() {
        fn system(meta: Res<RevMeta>, mut commands: Commands) {
            let not_log = meta.not_log();
            commands.as_rev(not_log).queue_undo_redo(blank_undo_redo);
            commands.queue(|world: &mut World| {
                world.trigger(Observer);
                world.spawn(OnAdd);
            });
        }

        let mut app = App::new();
        app.add_plugins(RevPlugin.set_runner_in_schedule(Update))
            // non-reversible systems should leak undo_redo into the next reversible system
            .add_systems(RevUpdate, system.before(RevSystems))
            .rev_add_systems(RevUpdate, system)
            .add_observer(observer)
            .add_observer(empty_observer);
        panic_on_warnings_or_errors(app.world_mut());
        app.update();
        let queue = app.world().resource::<UndoRedoQueue>();
        assert!(queue.is_empty(), "{queue:?}");
    }

    #[test]
    fn skipping_system_does_not_skip_redo() {
        #[derive(Resource, Default)]
        struct Counter(u8);

        fn system1(not_log: NotLog, mut commands: Commands) {
            commands
                .as_rev(not_log)
                .redo_and_queue(|world: &mut World, _: UndoRedoDirection| {
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

        let mut app = App::new();
        app.add_plugins(RevPlugin.set_runner_in_schedule(Update))
            .rev_add_systems(RevUpdate, (system1, system2, system3));
        panic_on_warnings_or_errors(app.world_mut());

        app.update();
        assert_eq!(app.world().resource::<Counter>().0, 3);

        RevQueue::RunBackwardLog.apply(app.world_mut());
        app.update();
        assert_eq!(app.world().resource::<Counter>().0, 6);

        RevQueue::RunForwardLog.apply(app.world_mut());
        app.update();
        assert_eq!(app.world().resource::<Counter>().0, 9);
    }

    #[test]
    fn queue_undo_redo_in_sync_works() {
        #[derive(Resource)]
        struct Done;

        fn system(not_log: NotLog, mut commands: Commands) {
            commands.queue(move |world: &mut World| {
                // this should be supported so users can write custom reversible commands where
                // the queues UndoRedo depends on what happens in the command
                world.commands().as_rev(not_log).rev_insert_resource(Done);
            });
        }

        let mut app = App::new();
        app.add_plugins(RevPlugin.set_runner_in_schedule(Update))
            .rev_add_systems(RevUpdate, system);
        panic_on_warnings_or_errors(app.world_mut());

        app.update();
        assert!(app.world().contains_resource::<Done>());

        RevQueue::RunBackwardLog.apply(app.world_mut());
        app.update();
        assert!(!app.world().contains_resource::<Done>());

        RevQueue::RunForwardLog.apply(app.world_mut());
        app.update();
        assert!(app.world().contains_resource::<Done>());
    }

    #[test]
    #[should_panic = "exclusive system"]
    fn deny_exclusive_systems() {
        super::into_rev_system(|_: &mut World| {});
    }
}

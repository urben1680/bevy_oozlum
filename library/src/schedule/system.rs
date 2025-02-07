use std::{
    any::TypeId,
    borrow::Cow,
    fmt::Debug,
    marker::PhantomData,
    sync::{Arc, Mutex, MutexGuard},
};

use bevy::{
    ecs::{
        archetype::ArchetypeComponentId,
        component::{ComponentId, Tick},
        query::Access,
        schedule::{InternedSystemSet, IntoSystemConfigs, IntoSystemSetConfigs, SystemSet},
        system::{IntoSystem, ReadOnlySystem, System, SystemIn},
        world::{unsafe_world_cell::UnsafeWorldCell, DeferredWorld, World},
    },
    utils::default,
};

use crate::{
    meta::RevMeta,
    schedule::{
        error_per_flag, BackwardSet, BwdCmdSet, BwdCmdSysSet, BwdSysSet, ForwardSet, FwdSysSet,
    },
    undo_redo::{RevBuffer, UndoRedoLog},
};

use super::{IntoRevSystemConfigs, RevSystemConfigs, RevSystemSetConfigs};

// needed to not collide with all_tuples!
#[doc(hidden)]
pub struct ReversibleSystem<Marker>(PhantomData<Marker>);

impl<Marker, T> IntoRevSystemConfigs<ReversibleSystem<Marker>> for T
where
    T: IntoSystem<(), (), Marker> + 'static,
{
    fn into_rev_configs(self) -> RevSystemConfigs {
        let system = IntoSystem::into_system(self);

        let sys_name = system.name();
        let name = |string: &str| {
            let mut name = String::with_capacity(sys_name.len() + string.len());
            name.extend([&sys_name, string]);
            name
        };
        let fwd_sys_name = name(" (forward system)");
        let bwd_sys_name = name(" (backward system)");
        let bwd_cmd_name = name(" (backward commands)");

        let default_system_sets: Vec<InternedSystemSet> = system.default_system_sets();

        let inner = Mutex::new(Inner {
            system,
            initialized: false,
            commands_log: default(),
        });

        let shared = Arc::new(Shared {
            inner,
            default_system_sets: default_system_sets.clone(),
        });

        let mut set_iter = default_system_sets.into_iter();
        let first_set = set_iter
            .next()
            .unwrap_or_else(|| panic!("System {sys_name} contais no default sets"));

        let mut forward_sys = ArcSystem {
            shared: shared.clone(),
            name: fwd_sys_name,
            tick: default(),
            is_send: default(),
            is_exclusive: default(),
            has_deferred: default(),
            forward: true,
            commands_err: false,
            component_access: default(),
            archetype_component_access: default(),
        }
        .in_set(FwdSysSet(first_set));

        let mut backward_cmd = CommandsBackward {
            shared: shared.clone(),
            name: bwd_cmd_name,
            tick: default(),
            has_deferred: default(),
            commands_err: false,
        }
        .in_set(BwdCmdSet(first_set));

        let mut backward_sys = ArcSystem {
            shared,
            name: bwd_sys_name,
            tick: default(),
            is_send: default(),
            is_exclusive: default(),
            has_deferred: default(),
            forward: false,
            commands_err: false,
            component_access: default(),
            archetype_component_access: default(),
        }
        .in_set(BwdSysSet(first_set));

        let mut fwd_sys_sets = FwdSysSet(first_set).into_configs();
        let mut bwd_cmd_sets = BwdCmdSet(first_set).in_set(BwdCmdSysSet(first_set));
        let mut bwd_sys_sets = BwdSysSet(first_set)
            .after(BwdCmdSet(first_set))
            .in_set(BwdCmdSysSet(first_set));
        let mut bwd_cmd_sys_sets = BwdCmdSysSet(first_set).into_configs();

        for set in set_iter {
            forward_sys.in_set_inner(FwdSysSet(set).intern());
            backward_cmd.in_set_inner(BwdCmdSet(set).intern());
            backward_sys.in_set_inner(BwdSysSet(set).intern());

            fwd_sys_sets = (fwd_sys_sets, FwdSysSet(set)).into_configs();
            bwd_cmd_sets = (bwd_cmd_sets, BwdCmdSet(set).in_set(BwdCmdSysSet(set))).into_configs();
            bwd_sys_sets = (
                bwd_sys_sets,
                BwdSysSet(set)
                    .after(BwdCmdSet(set))
                    .in_set(BwdCmdSysSet(set)),
            )
                .into_configs();
            bwd_cmd_sys_sets = (bwd_cmd_sys_sets, BwdCmdSysSet(set)).into_configs();
        }

        // Note that System::has_deferred may return no correct value before initializing the system.
        // Because of this and that initializing the system here might be surprising for the user
        // the CommandsBackward system is always added. it becomes noop if the system ends up having no
        // deferred buffers. CommandsBackward::has_deferred returns the value of the actual system.
        RevSystemConfigs {
            systems: (forward_sys, backward_cmd, backward_sys).into_configs(),
            sets: RevSystemSetConfigs {
                fwd_sys_sets: fwd_sys_sets.in_set(ForwardSet),
                bwd_cmd_sets,
                bwd_sys_sets,
                bwd_cmd_sys_sets: bwd_cmd_sys_sets.in_set(BackwardSet),
                condition_sets: ForwardSet.into_configs(),
            },
        }
    }
}

struct ArcSystem<T> {
    shared: Arc<Shared<T>>,
    name: String,
    tick: Tick,
    forward: bool,
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

impl<T: System> System for ArcSystem<T> {
    type In = <T as System>::In;
    type Out = <T as System>::Out;

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
    unsafe fn validate_param_unsafe(&mut self, world: UnsafeWorldCell) -> bool {
        // commands log needs to read RevMeta
        if self.is_exclusive && !world.get_resource::<RevMeta>().is_some() {
            return false;
        }
        self.shared
            .inner
            .try_lock()
            .unwrap_or_else(expect_lock(&self.name))
            .system
            .validate_param_unsafe(world)
    }
    fn validate_param(&mut self, world: &World) -> bool {
        // commands log needs to read RevMeta
        if self.is_exclusive && !world.contains_resource::<RevMeta>() {
            return false;
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
        assert!(
            !self.is_exclusive,
            "expected scheduler to use System::run for exclusive system"
        );
        self.shared
            .inner
            .try_lock()
            .unwrap_or_else(expect_lock(&self.name))
            .system
            .run_unsafe(input, world)
    }
    fn run(&mut self, input: SystemIn<'_, Self>, world: &mut World) -> Self::Out {
        let mut shared = self
            .shared
            .inner
            .try_lock()
            .unwrap_or_else(expect_lock(&self.name));

        if !self.is_exclusive() {
            shared.system.run(input, world)
        } else if self.forward {
            let out = shared.system.run(input, world);
            if let Err(err) = shared.commands_log.forward(world, &self.name) {
                error_per_flag!(
                    &mut self.commands_err,
                    "Reversible commands from reversible exclusive system {} could not be done/redone: {err:#?}",
                    self.name
                )
            }
            out
        } else {
            if let Err(err) = shared.commands_log.backward(world, &self.name) {
                error_per_flag!(
                    &mut self.commands_err,
                    "Reversible commands from reversible exclusive system {} could not be undone: {err:#?}",
                    self.name
                )
            }
            shared.system.run(input, world)
        }
    }
    fn apply_deferred(&mut self, world: &mut World) {
        if !self.has_deferred || self.is_exclusive {
            // exclusive systems have an empty body in this trait method, see ExclusiveFunctionSystem
            return;
        }

        let mut shared = self
            .shared
            .inner
            .try_lock()
            .unwrap_or_else(expect_lock(&self.name));

        shared.system.apply_deferred(world);

        if !self.forward {
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
        let shared = initialize_arc_system(&mut self.shared, &mut self.tick, &self.name, world);
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
unsafe impl<T: ReadOnlySystem> ReadOnlySystem for ArcSystem<T> {
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
    unsafe fn validate_param_unsafe(&mut self, world: UnsafeWorldCell) -> bool {
        // noop if has no deferred
        if !self.has_deferred {
            return false;
        }
        // keep symmetry
        self.shared
            .inner
            .try_lock()
            .unwrap_or_else(expect_lock(&self.name))
            .system
            .validate_param_unsafe(world)
    }
    fn validate_param(&mut self, world: &World) -> bool {
        // noop if has no deferred
        if !self.has_deferred {
            return false;
        }
        // keep symmetry
        self.shared
            .inner
            .try_lock()
            .unwrap_or_else(expect_lock(&self.name))
            .system
            .validate_param(world)
    }
    unsafe fn run_unsafe(&mut self, _input: (), _world: UnsafeWorldCell) {}
    fn run(&mut self, _input: (), _world: &mut World) {}
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
        let shared = initialize_arc_system(&mut self.shared, &mut self.tick, &self.name, world);
        self.has_deferred = shared.system.has_deferred();
    }
    fn update_archetype_component_access(&mut self, _world: UnsafeWorldCell) {}
}

// SAFETY: noop run_readonly
unsafe impl<T: System> ReadOnlySystem for CommandsBackward<T> {
    fn run_readonly(&mut self, _input: (), _world: &World) {}
}

fn initialize_arc_system<'a, T: System>(
    shared: &'a mut Arc<Shared<T>>,
    tick: &mut Tick,
    name: &String,
    world: &mut World,
) -> MutexGuard<'a, Inner<T>> {
    world.init_resource::<RevBuffer>();
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
            schedule::IntoSystemConfigs,
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
        assert!(app.world().resource::<RevBuffer>().undo_redo_is_empty());
    }

    #[test]
    fn non_exclusive_system_drains_all_undo_redo() {
        assert_system_drains_all_undo_redo(
            |mut buffer: ResMut<RevBuffer>, mut commands: Commands| {
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

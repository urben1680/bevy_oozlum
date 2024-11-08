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
        schedule::{InternedSystemSet, IntoSystemConfigs, IntoSystemSetConfigs},
        system::{IntoSystem, ReadOnlySystem, System},
        world::{unsafe_world_cell::UnsafeWorldCell, DeferredWorld, World},
    },
    utils::default,
};

use crate::{
    check_tick,
    commands::CommandsLog,
    error_per_flag,
    meta::CommandsLogReducings,
    schedule::{BwdArcSet, BwdCmdArcSet, FwdArcSet},
};

use super::{IntoRevSystemConfigs, RevSystemConfigs, RevSystemSetConfigs};

#[doc(hidden)]
pub struct ReversibleSystem<Marker>(PhantomData<Marker>);

impl<Marker, T> IntoRevSystemConfigs<ReversibleSystem<Marker>> for T
where
    T: IntoSystem<(), (), Marker> + 'static,
{
    fn into_rev_configs(self) -> RevSystemConfigs {
        let system = IntoSystem::into_system(self);

        let name = |string: &str| {
            let mut name = String::with_capacity(system.name().len() + string.len());
            name.extend([&system.name(), string]);
            name
        };
        let fwd_sys_name = name(" (forward system)");
        let bwd_sys_name = name(" (backward system)");
        let bwd_cmd_name = name(" (backward commands)");
        let observer_name = name(" (backward commands observer)");

        let default_system_sets = system.default_system_sets();

        let shared = Mutex::new(SharedMut {
            system,
            initialized: false,
            commands_log: default(),
            observer_name,
        });

        let shared = Arc::new(Shared {
            inner: shared,
            default_system_sets,
        });

        let forward_sys = ArcSystem {
            shared: shared.clone(),
            name: fwd_sys_name,
            tick: Tick::new(0),
            is_send: false,
            is_exclusive: false,
            has_deferred: false,
            forward: true,
            commands_err: false,
            component_access: default(),
            archetype_component_access: default(),
        };

        let backward_sys = ArcSystem {
            shared: shared.clone(),
            name: bwd_sys_name,
            tick: Tick::new(0),
            is_send: false,
            is_exclusive: false,
            has_deferred: false,
            forward: false,
            commands_err: false,
            component_access: default(),
            archetype_component_access: default(),
        };

        let backward_cmd = CommandsBackward {
            shared: shared.clone(),
            name: bwd_cmd_name,
            tick: Tick::new(0),
            has_deferred: false,
            commands_err: false,
        };

        let id = TypeId::of::<T::System>();

        // Note that System::has_deferred may return no correct value before initializing the system.
        // Because of this and that initializing the system here might be surprising for the user
        // the CommandsBackward system is always added. it becomes noop if the system ends up having no
        // deferred buffers. CommandsBackward::has_deferred returns the value of the actual system.
        RevSystemConfigs {
            systems: (
                forward_sys.in_set(FwdArcSet(id)),
                (backward_cmd, backward_sys.in_set(BwdArcSet(id)))
                    .chain()
                    .in_set(BwdCmdArcSet(id)),
            )
                .into_configs(),
            sets: RevSystemSetConfigs {
                fwd_arc_sets: FwdArcSet(id).into_configs(),
                bwd_cmd_arc_sets: BwdCmdArcSet(id).into_configs(),
                bwd_arc_sets: BwdArcSet(id).into_configs(),
            },
        }
    }
}

struct SharedMut<T> {
    system: T,
    initialized: bool,
    commands_log: CommandsLog,
    observer_name: String,
}

struct Shared<T> {
    inner: Mutex<SharedMut<T>>,
    default_system_sets: Vec<InternedSystemSet>,
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

    // these need to be cloned because one cannot return &'a Access from owned RwLockReadGuard<'a, Access>
    component_access: Access<ComponentId>,
    archetype_component_access: Access<ArchetypeComponentId>,
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
    unsafe fn run_unsafe(&mut self, input: Self::In, world: UnsafeWorldCell) -> Self::Out {
        debug_assert!(
            !self.is_exclusive(),
            "expected scheduler to use System::run for exclusive system"
        );
        self.shared
            .inner
            .try_lock()
            .unwrap_or_else(expect_shared(&self.name))
            .system
            .run_unsafe(input, world)
    }
    fn run(&mut self, input: Self::In, world: &mut World) -> Self::Out {
        let mut shared = self
            .shared
            .inner
            .try_lock()
            .unwrap_or_else(expect_shared(&self.name));

        if !self.is_exclusive() {
            shared.system.run(input, world)
        } else if self.forward {
            let out = shared.system.run(input, world);
            if let Err(err) = shared.commands_log.forward(world) {
                error_per_flag!(
                    &mut self.commands_err,
                    "Reversible commands from reversible system {} could not be done/redone: {err:?}",
                    self.name
                )
            }
            out
        } else {
            if let Err(err) = shared.commands_log.backward(world) {
                error_per_flag!(
                    &mut self.commands_err,
                    "Reversible commands from reversible system {} could not be undone: {err:?}",
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
            .unwrap_or_else(expect_shared(&self.name));

        shared.system.apply_deferred(world);

        // make sure everything is done, expect that all hooks and observers ran too
        // todo: remove this if it is not needed
        world.flush();

        if !self.forward {
            return;
        }

        // reverisble commands are now in the buffer resource so commands_log can take them
        if let Err(err) = shared.commands_log.forward(world) {
            error_per_flag!(
                &mut self.commands_err,
                "Reversible commands from reversible system {} could not be done/redone: {err:?}",
                self.name
            )
        }
    }
    fn queue_deferred(&mut self, _world: DeferredWorld) {
        unreachable!("{} used as an observer", std::any::type_name::<T>())
    }
    fn initialize(&mut self, world: &mut World) {
        let shared = initialize_arc_system(&mut self.shared, &mut self.tick, &self.name, world);
        self.is_send = shared.system.is_send();
        self.is_exclusive = shared.system.is_exclusive();
        self.has_deferred = shared.system.has_deferred();
    }
    fn update_archetype_component_access(&mut self, world: UnsafeWorldCell) {
        // reference: CombinatorSystem
        let system = &mut self
            .shared
            .inner
            .try_lock()
            .unwrap_or_else(expect_shared(&self.name))
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
    fn run_readonly(&mut self, input: Self::In, world: &World) -> Self::Out {
        self.shared
            .inner
            .try_lock()
            .unwrap_or_else(expect_shared(&self.name))
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
    fn apply_deferred(&mut self, world: &mut World) {
        let result = self
            .shared
            .inner
            .try_lock()
            .unwrap_or_else(expect_shared(&self.name))
            .commands_log
            .backward(world);
        if let Err(err) = result {
            error_per_flag!(
                &mut self.commands_err,
                "Reversible commands from reversible system {} could not be undone: {err:?}",
                self.name
            )
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
    fn run(&mut self, _input: (), _world: &mut World) {}
    unsafe fn run_unsafe(&mut self, _input: (), _world: UnsafeWorldCell) {}
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
) -> MutexGuard<'a, SharedMut<T>> {
    *tick = world.change_tick();
    let arc = shared.clone();
    let mut shared = shared.inner.try_lock().unwrap_or_else(expect_shared(name));
    if shared.initialized {
        return shared;
    }

    // init system
    shared.system.initialize(world);
    shared.initialized = true;

    // add observer for reducing commands using the logged_at mechanism
    let name = shared.observer_name.clone();
    world
        .get_resource_or_insert_with(CommandsLogReducings::default)
        .0
        .push(Box::new(move |meta, world| {
            arc.inner
                .try_lock()
                .unwrap_or_else(expect_shared(&name))
                .commands_log
                .reduce_logged_at(world, meta)
        }));

    shared
}

fn expect_shared<T: Debug, Out>(name: &String) -> impl FnOnce(T) -> Out + '_ {
    move |err| {
        panic!("Could not access reversible system {name} because of {err:?}. This is a crate bug.")
    }
}

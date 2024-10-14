use std::{
    any::TypeId,
    borrow::Cow,
    fmt::Debug,
    marker::PhantomData,
    sync::{Arc, RwLock},
};

use bevy::{ecs::{
    archetype::ArchetypeComponentId,
    component::{ComponentId, Tick},
    query::Access,
    schedule::{InternedSystemSet, IntoSystemConfigs, SystemConfigs, SystemSet},
    system::{IntoSystem, ReadOnlySystem, System},
    world::{unsafe_world_cell::UnsafeWorldCell, DeferredWorld, World},
}, utils::default};

use crate::{
    check_tick, commands::CommandsLog, error_per_flag, meta::CommandsLogReducingBox,
    set_configs::RevSystemSetConfigs, BackwardCmdsSys, BackwardSys,
};

use super::{IntoRevSystemConfigs, RevSystemConfigs};

struct ReversibleSystem<Marker>(PhantomData<Marker>);

#[derive(SystemSet, Copy, Clone, Debug, Hash, PartialEq, Eq)]
struct ArcSystemFallbackSet(TypeId);

impl<Marker, T> IntoRevSystemConfigs<ReversibleSystem<Marker>> for T
where
    T: IntoSystem<(), (), Marker>,
{
    fn into_rev_configs(self) -> RevSystemConfigs {
        let system = IntoSystem::into_system(self);

        let mut sets = system.default_system_sets();
        let fallback_set = ArcSystemFallbackSet(system.type_id());
        if sets.is_empty() {
            sets = vec![fallback_set.intern()];
        }

        let name = |string: &str| {
            let mut name = String::with_capacity(system.name().len() + string.len());
            name.extend([&system.name(), string]);
            name
        };
        let fwd_sys_name = name(" (forward)");
        let bwd_sys_name = name(" (backward)");
        let bwd_cmd_name = name(" (backward commands)");
        let observer_name = name(" (observer)");

        let shared = Arc::new(RwLock::new(Shared {
            system,
            initialized: false,
            commands_log: default(),
        }));

        let forward_sys = ArcSystem {
            shared: shared.clone(),
            name: fwd_sys_name,
            tick: Tick::new(0),
            commands_forward_err: Some(false),
            component_access: default(),
            archetype_component_access: default(),
        };

        let backward_sys = ArcSystem {
            shared: shared.clone(),
            name: bwd_sys_name,
            tick: Tick::new(0),
            commands_forward_err: None,
            component_access: default(),
            archetype_component_access: default(),
        };

        let backward_cmd = CommandsBackward {
            shared: shared.clone(),
            name: bwd_cmd_name,
            tick: Tick::new(0),
            commands_err: false,
        };

        let forward = match sets.is_empty() {
            true => forward_sys.in_set(fallback_set),
            false => forward_sys.into_configs(),
        };
        let mut backward = config_in_sets(BackwardSys, &sets, backward_sys);
        backward = (backward_cmd, backward).chain();
        backward = config_in_sets(BackwardCmdsSys, &sets, backward);
        let set_configs = RevSystemSetConfigs::from_sets(sets).unwrap(); // sets not empty

        let observer: CommandsLogReducingBox = Box::new(move |event, world| {
            shared
                .try_write()
                .unwrap_or_else(expect_shared(&observer_name))
                .commands_log
                .reduce_logged_at(world, event);
        });

        // Note that System::has_deferred may return no correct value before initializing the system.
        // Because of this and that initializing the system here might be surprising for the user
        // the CommandsBackward system is always added. it becomes noop if the system ends up having no
        // deferred buffers. CommandsBackward::has_deferred returns the value of the actual system.
        RevSystemConfigs {
            forward,
            backward,
            set_configs,
            commands_logged_at_reductions: vec![observer],
        }
    }
}

struct Shared<T> {
    system: T,
    initialized: bool,
    commands_log: CommandsLog,
}

struct ArcSystem<T> {
    shared: Arc<RwLock<Shared<T>>>,
    name: String,
    tick: Tick,
    commands_forward_err: Option<bool>,

    // these need to be cloned because one cannot return &'a Access from RwLockReadGuard<'a, Access>
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
        self.shared
            .try_read()
            .unwrap_or_else(expect_shared(&self.name))
            .system
            .is_send()
    }
    fn is_exclusive(&self) -> bool {
        self.shared
            .try_read()
            .unwrap_or_else(expect_shared(&self.name))
            .system
            .is_exclusive()
    }
    fn has_deferred(&self) -> bool {
        self.shared
            .try_read()
            .unwrap_or_else(expect_shared(&self.name))
            .system
            .has_deferred()
    }
    unsafe fn run_unsafe(&mut self, input: Self::In, world: UnsafeWorldCell) -> Self::Out {
        self.shared
            .try_write()
            .unwrap_or_else(expect_shared(&self.name))
            .system
            .run_unsafe(input, world)
    }
    fn run(&mut self, input: Self::In, world: &mut World) -> Self::Out {
        self.shared
            .try_write()
            .unwrap_or_else(expect_shared(&self.name))
            .system
            .run(input, world)
    }
    fn apply_deferred(&mut self, world: &mut World) {
        let mut shared = self
            .shared
            .try_write()
            .unwrap_or_else(expect_shared(&self.name));

        shared.system.apply_deferred(world);

        // make sure everything is done, expect that all hooks and observers ran too
        // todo: remove this if it is not needed
        world.flush();

        if let Some(commands_err) = self.commands_forward_err.as_mut() {
            // reverisble commands are now in the buffer resource so commands_log can take them
            if let Err(err) = shared.commands_log.forward(world) {
                error_per_flag!(
                    commands_err,
                    "Reversible commands from reversible system {} could not be done/redone: {err:?}",
                    self.name
                )
            }
        }
    }
    fn queue_deferred(&mut self, world: DeferredWorld) {
        self.shared
            .try_write()
            .unwrap_or_else(expect_shared(&self.name))
            .system
            .queue_deferred(world);
        // todo deferred -> reversible observer?
    }
    fn initialize(&mut self, world: &mut World) {
        initialize_arc_system(&mut self.shared, &mut self.tick, &self.name, world);
    }
    fn update_archetype_component_access(&mut self, world: UnsafeWorldCell) {
        // reference: CombinatorSystem
        let system = &mut self
            .shared
            .try_write()
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
        self.shared
            .try_read()
            .unwrap_or_else(expect_shared(&self.name))
            .system
            .default_system_sets()
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
            .try_write()
            .unwrap_or_else(expect_shared(&self.name))
            .system
            .run_readonly(input, world)
    }
}

struct CommandsBackward<T> {
    shared: Arc<RwLock<Shared<T>>>,
    name: String,
    tick: Tick,
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
        self.shared
            .try_read()
            .unwrap_or_else(expect_shared(&self.name))
            .system
            .has_deferred()
    }
    fn apply_deferred(&mut self, world: &mut World) {
        let result = self
            .shared
            .try_write()
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
        unreachable!("not used as an observer log")
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
        #[derive(SystemSet, Copy, Clone, Debug, Hash, PartialEq, Eq)]
        struct Set(TypeId);
        vec![Set(TypeId::of::<Self>()).intern()]
    }
    fn initialize(&mut self, world: &mut World) {
        initialize_arc_system(&mut self.shared, &mut self.tick, &self.name, world);
    }
    fn run(&mut self, _input: (), _world: &mut World) {}
    unsafe fn run_unsafe(&mut self, _input: (), _world: UnsafeWorldCell) {}
    fn update_archetype_component_access(&mut self, _world: UnsafeWorldCell) {}
}

// SAFETY: noop run_readonly
unsafe impl<T: System> ReadOnlySystem for CommandsBackward<T> {
    fn run_readonly(&mut self, _input: (), _world: &World) {}
}

fn initialize_arc_system(
    shared: &mut Arc<RwLock<Shared<impl System>>>,
    tick: &mut Tick,
    name: &String,
    world: &mut World,
) {
    *tick = world.change_tick();
    let mut shared = shared.try_write().unwrap_or_else(expect_shared(name));
    if !shared.initialized {
        shared.system.initialize(world);
        shared.initialized = true;
    }
}

fn expect_shared<T: Debug, Out>(name: &String) -> impl FnOnce(T) -> Out + '_ {
    move |err| {
        panic!("Could not access reversible system {name} because of {err:?}. This is a crate bug.")
    }
}

fn config_in_sets<Marker, Set: SystemSet>(
    map: impl Fn(InternedSystemSet) -> Set,
    sets: &Vec<InternedSystemSet>,
    config: impl IntoSystemConfigs<Marker>,
) -> SystemConfigs {
    let mut configs = config.into_configs();
    for set in sets {
        configs = configs.in_set(map(*set));
    }
    configs
}

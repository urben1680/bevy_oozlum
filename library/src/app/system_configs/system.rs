use std::{
    any::TypeId,
    borrow::Cow,
    sync::{Arc, Mutex, RwLock},
};

use bevy::ecs::{
    archetype::ArchetypeComponentId,
    component::{ComponentId, Tick},
    query::Access,
    schedule::{InternedSystemSet, IntoSystemConfigs, IntoSystemSetConfigs, SystemSet},
    system::{IntoSystem, ReadOnlySystem, System},
    world::{unsafe_world_cell::UnsafeWorldCell, DeferredWorld, World},
};

use crate::{
    app::{
        check_tick, set_configs::RevSystemSetConfigs, BackwardCmdsSys, BackwardSys,
        EMPTY_ARCHETYPE_COMPONENT_ACCESS, EMPTY_COMPONENT_ACCESS,
    },
    commands::CommandsLog,
};

use super::{IntoRevSystemConfigs, RevSystemConfigs};

impl<Marker, T> IntoRevSystemConfigs<ArcSystem<Marker>> for T
where
    T: IntoSystem<(), (), Marker>,
{
    fn into_rev_configs(self) -> RevSystemConfigs {
        let system = IntoSystem::into_system(self);
        let sets = system.default_system_sets();
        assert_eq!(
            sets.len(),
            1,
            "expected system.default_system_sets() to only return one default set"
        );
        let fwd_set = sets[0].intern();
        let bwd_cmds_sys_set = BackwardCmdsSys(sets[0]).intern();
        let bwd_sys_set = BackwardSys(sets[0]).intern();

        let name = |string: &str| {
            let mut name = String::with_capacity(system.name().len() + string.len());
            name.extend([&system.name(), string]);
            name
        };
        let fwd_sys_name = name(" (forward)");
        let bwd_sys_name = name(" (backward)");
        let fwd_cmd_name = name(" (forward commands)");
        let bwd_cmd_name = name(" (backward commands)");

        let system_and_initialized = RwLock::new((system, false));
        let system_and_initialized = Arc::new(system_and_initialized);
        let commands_log: Arc<Mutex<CommandsLog>> = Default::default();

        let fwd_sys = ArcSystem {
            system_and_initialized: system_and_initialized.clone(),
            name: fwd_sys_name,
            tick: Tick::new(0),
            component_access: Default::default(),
            archetype_component_access: Default::default(),
        };

        let bwd_sys = ArcSystem {
            system_and_initialized: system_and_initialized.clone(),
            name: bwd_sys_name,
            tick: Tick::new(0),
            component_access: Default::default(),
            archetype_component_access: Default::default(),
        };

        let fwd_cmd = CommandsForward {
            name: fwd_cmd_name,
            log: commands_log.clone(),
        };

        let bwd_cmd = CommandsBackward {
            system_and_initialized,
            name: bwd_cmd_name,
            log: commands_log,
            tick: Tick::new(0),
        };

        RevSystemConfigs {
            forward: fwd_sys.pipe(fwd_cmd).into_configs(),
            backward: (bwd_cmd, bwd_sys.in_set(bwd_sys_set))
                .in_set(bwd_cmds_sys_set)
                .chain(),
            set_configs: RevSystemSetConfigs {
                forward_sys: fwd_set.into_configs(),
                backward_cmds_sys: bwd_cmds_sys_set.into_configs(),
                backward_sys: bwd_sys_set.into_configs(),
            },
        }
    }
}

// todo: exemplary, replace with macros
impl<S0, M0, S1, M1> IntoRevSystemConfigs<((S0, M0), (S1, M1))> for (S0, S1)
where
    S0: IntoRevSystemConfigs<M0>,
    S1: IntoRevSystemConfigs<M1>,
{
    fn into_rev_configs(self) -> RevSystemConfigs {
        let configs0 = self.0.into_rev_configs();
        let configs1 = self.1.into_rev_configs();
        RevSystemConfigs {
            forward: (configs0.forward, configs1.forward).into_configs(),
            backward: (configs1.backward, configs0.backward).into_configs(), // reverse order for consitency, not actually needed as it remains unconfigured
            set_configs: RevSystemSetConfigs {
                forward_sys: (
                    configs0.set_configs.forward_sys,
                    configs1.set_configs.forward_sys,
                )
                    .into_configs(),
                backward_cmds_sys: (
                    // reverse order!
                    configs1.set_configs.backward_cmds_sys,
                    configs0.set_configs.backward_cmds_sys,
                )
                    .into_configs(),
                backward_sys: (
                    // reverse order!
                    configs1.set_configs.backward_sys,
                    configs0.set_configs.backward_sys,
                )
                    .into_configs(),
            },
        }
    }
}

struct ArcSystem<T> {
    system_and_initialized: Arc<RwLock<(T, bool)>>,
    name: String,
    component_access: Access<ComponentId>,
    archetype_component_access: Access<ArchetypeComponentId>,
    tick: Tick,
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
        // needs mirrored field because returning &'a A from RwLockReadGuard<'a, A> triggers error[E0515]
        &self.component_access
    }
    fn archetype_component_access(&self) -> &Access<ArchetypeComponentId> {
        // needs mirrored field because returning &'a A from RwLockReadGuard<'a, A> triggers error[E0515]
        &self.archetype_component_access
    }
    fn is_send(&self) -> bool {
        self.system_and_initialized.try_read().unwrap().0.is_send()
    }
    fn is_exclusive(&self) -> bool {
        self.system_and_initialized
            .try_read()
            .unwrap()
            .0
            .is_exclusive()
    }
    fn has_deferred(&self) -> bool {
        self.system_and_initialized
            .try_read()
            .unwrap()
            .0
            .has_deferred()
    }
    unsafe fn run_unsafe(&mut self, input: Self::In, world: UnsafeWorldCell) -> Self::Out {
        self.system_and_initialized
            .try_write()
            .unwrap()
            .0
            .run_unsafe(input, world)
    }
    fn run(&mut self, input: Self::In, world: &mut World) -> Self::Out {
        self.system_and_initialized
            .try_write()
            .unwrap()
            .0
            .run(input, world)
    }
    fn apply_deferred(&mut self, world: &mut World) {
        self.system_and_initialized
            .try_write()
            .unwrap()
            .0
            .apply_deferred(world)
    }
    fn queue_deferred(&mut self, world: DeferredWorld) {
        self.system_and_initialized
            .try_write()
            .unwrap()
            .0
            .queue_deferred(world)
    }
    fn initialize(&mut self, world: &mut World) {
        initialize_arc_system(&mut self.system_and_initialized, &mut self.tick, world);
    }
    fn update_archetype_component_access(&mut self, world: UnsafeWorldCell) {
        // reference: CombinatorSystem
        let system = &mut self.system_and_initialized.try_write().unwrap().0;
        system.update_archetype_component_access(world);
        self.archetype_component_access
            .extend(system.archetype_component_access());
    }
    fn check_change_tick(&mut self, change_tick: Tick) {
        check_tick(&mut self.tick, change_tick);
    }
    fn default_system_sets(&self) -> Vec<InternedSystemSet> {
        self.system_and_initialized
            .try_read()
            .unwrap()
            .0
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
        self.system_and_initialized
            .try_write()
            .unwrap()
            .0
            .run_readonly(input, world)
    }
}

struct CommandsForward {
    name: String,
    log: Arc<Mutex<CommandsLog>>,
}

struct CommandsBackward<T> {
    system_and_initialized: Arc<RwLock<(T, bool)>>,
    name: String,
    log: Arc<Mutex<CommandsLog>>,
    tick: Tick,
}

impl System for CommandsForward {
    type In = ();
    type Out = ();
    fn name(&self) -> Cow<'static, str> {
        Cow::Owned(self.name.clone())
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
        // If the user system returns false, then it contains no Commands, therefore cannot issue reversible commands and
        // CommandsForward would not redo such commands. So if `bevy_ecs::system::combinator::CombinatorSystem::has_deferred`
        // evaluates `system.has_deferred() || CommandsForward::has_deferred(_)`, the system value should decide if the pipe
        // has deferred as well.
        false
    }
    fn apply_deferred(&mut self, world: &mut World) {
        // at this line the previous system already applied reversible commands and the data waits to be taken from the buffer resource.
        // see `bevy_ecs::system::combinator::CombinatorSystem::apply_deferred`
        self.log.try_lock().expect("todo").forward(world);
    }
    fn queue_deferred(&mut self, _world: DeferredWorld) {
        unreachable!("not used as an observer log")
    }
    fn check_change_tick(&mut self, _change_tick: Tick) {}
    fn get_last_run(&self) -> Tick {
        unreachable!("Expected CombinatorSystem to return the tick from the piped-in system")
    }
    fn set_last_run(&mut self, _last_run: Tick) {}
    fn default_system_sets(&self) -> Vec<InternedSystemSet> {
        Vec::new()
    }
    fn initialize(&mut self, _world: &mut World) {}
    fn run(&mut self, _input: (), _world: &mut World) {}
    unsafe fn run_unsafe(&mut self, _input: (), _world: UnsafeWorldCell) {}
    fn update_archetype_component_access(&mut self, _world: UnsafeWorldCell) {}
}

// SAFETY: noop run_readonly
unsafe impl ReadOnlySystem for CommandsForward {
    fn run_readonly(&mut self, _input: (), _world: &World) {}
}

impl<T: System> System for CommandsBackward<T> {
    type In = ();
    type Out = ();
    fn name(&self) -> Cow<'static, str> {
        Cow::Owned(self.name.clone())
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
        self.system_and_initialized
            .try_read()
            .unwrap()
            .0
            .has_deferred()
    }
    fn apply_deferred(&mut self, world: &mut World) {
        self.log.try_lock().expect("todo").backward(world)
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
        initialize_arc_system(&mut self.system_and_initialized, &mut self.tick, world);
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
    system_and_initialized: &mut Arc<RwLock<(impl System, bool)>>,
    tick: &mut Tick,
    world: &mut World,
) {
    *tick = world.change_tick();
    let mut system_and_initialized = system_and_initialized.try_write().unwrap();
    if !system_and_initialized.1 {
        system_and_initialized.1 = true;
        system_and_initialized.0.initialize(world);
    }
}

use std::{any::TypeId, borrow::Cow};

use bevy::{
    ecs::{
        archetype::ArchetypeComponentId,
        component::{ComponentId, Tick},
        query::Access,
        resource::Resource,
        schedule::{Condition, InternedSystemSet, IntoScheduleConfigs, ScheduleConfigs},
        system::{IntoSystem, ReadOnlySystem, Res, System, SystemIn, SystemParamValidationError},
        world::{unsafe_world_cell::UnsafeWorldCell, DeferredWorld, World},
    },
    utils::default,
};

use crate::{
    log::{OutOfLog, SparseTransitionLog},
    meta::{RevDirection, RevMeta},
    schedule::error_per_flag,
};

use super::AtomicSet;

pub(super) fn rev_condition<Marker>(
    condition: impl Condition<Marker>,
) -> (InternedSystemSet, ScheduleConfigs<InternedSystemSet>) {
    let condition = IntoSystem::into_system(condition);
    let name = condition.name();
    let condition = RevCondition {
        condition,
        meta_id: default(),
        log: default(),
        component_access: default(),
        archetype_component_access: default(),
        out_of_log_err: false,
    };
    let set = AtomicSet::new(name);
    (set, set.run_if(condition))
}

struct RevCondition<T> {
    condition: T,
    meta_id: Option<ComponentId>,
    log: SparseTransitionLog<()>,
    // needs its own Access to register RevMeta read
    component_access: Access<ComponentId>,
    archetype_component_access: Access<ArchetypeComponentId>,

    out_of_log_err: bool,
}

impl<T: ReadOnlySystem<Out = bool>> System for RevCondition<T> {
    type In = T::In;
    type Out = bool;
    fn name(&self) -> Cow<'static, str> {
        self.condition.name()
    }
    fn type_id(&self) -> TypeId {
        self.condition.type_id()
    }
    fn component_access(&self) -> &Access<ComponentId> {
        &self.component_access
    }
    fn archetype_component_access(&self) -> &Access<ArchetypeComponentId> {
        &self.archetype_component_access
    }
    fn is_send(&self) -> bool {
        self.condition.is_send()
    }
    fn is_exclusive(&self) -> bool {
        self.condition.is_exclusive()
    }
    fn has_deferred(&self) -> bool {
        self.condition.has_deferred()
    }
    fn initialize(&mut self, world: &mut World) {
        /// Not everything of the bevy API that is needed here to update archetype_component_access is public,
        /// so this is a rather complicated way to do it while trying to make it cheap after the first call.
        /// The benefit is that this is agnostic to implementation details of how impl SystemParam for Res works.
        #[derive(Resource)]
        struct RevMetaAccesses {
            component_access: Access<ComponentId>,
            archarchetype_component_access: Access<ArchetypeComponentId>,
        }

        self.condition.initialize(world);
        self.meta_id = Some(world.register_resource::<RevMeta>());
        self.component_access
            .extend(self.condition.component_access());
        self.archetype_component_access
            .extend(self.condition.archetype_component_access());

        let access = match world.get_resource::<RevMetaAccesses>() {
            Some(access) => access,
            None => {
                let mut system = IntoSystem::into_system(|_: Res<RevMeta>| {});
                system.initialize(world);
                world.insert_resource(RevMetaAccesses {
                    component_access: system.component_access().clone(),
                    archarchetype_component_access: system.archetype_component_access().clone(),
                });
                world.resource::<RevMetaAccesses>()
            }
        };

        if access
            .component_access
            .is_compatible(&self.component_access)
        {
            self.component_access.extend(&access.component_access);
        }
        if access
            .archarchetype_component_access
            .is_compatible(&self.archetype_component_access)
        {
            self.archetype_component_access
                .extend(&access.archarchetype_component_access);
        }
    }
    unsafe fn validate_param_unsafe(
        &mut self,
        world: UnsafeWorldCell,
    ) -> Result<(), SystemParamValidationError> {
        // SAFETY:
        // - Registered read access to resource
        // - T cannot have write access because T: ReadOnlySystem
        world
            .get_resource_by_id(self.meta_id.unwrap())
            .ok_or(SystemParamValidationError::invalid::<Self>(
                RevMeta::EXPECT_IN_WORLD,
            ))?
            .deref::<RevMeta>() // SAFETY: todo
            .get_running_direction()
            .ok_or(SystemParamValidationError::invalid::<Self>(
                RevMeta::EXPECT_RUNNING,
            ))
            .and_then(|_| self.condition.validate_param_unsafe(world))
    }
    unsafe fn run_unsafe(&mut self, input: SystemIn<'_, Self>, world: UnsafeWorldCell) -> bool {
        let meta = unsafe {
            // SAFETY:
            // - Registered read access to resource
            // - T cannot have write access because T: ReadOnlySystem
            world.get_resource_by_id(self.meta_id.unwrap())
        }
        .expect("Self::validate_param ensured Some");
        // SAFETY:
        // todo
        let meta = meta.deref::<RevMeta>();
        match meta.running_direction() {
            RevDirection::NOT_LOG => {
                let out = self.condition.run_unsafe(input, world);
                self.log.push_and_pop_past(
                    meta.past_len() as usize,
                    out.then_some(())
                );
                out
            },
            // todo, simplify error msg, can only be internal bug
            RevDirection::FORWARD_LOG => {
                match self.log.forward_log() {
                    Ok(option) => option.is_some(),
                    Err(OutOfLog) => error_per_flag!(&mut self.out_of_log_err, "Reversible condition {} got out of log. \
                        Make sure the reversible schedule this condition is in is correctly called in both the forward and backward direction. \
                        It seems that one or more backward schedule calls were missed. \
                        If this condition is in the RevUpdate schedule, this is likely a crate bug.\n{meta:?}", self.name())
                }
            },
            RevDirection::BackwardLog => {
                match self.log.backward_log() {
                    Ok(option) => option.is_some(),
                    Err(OutOfLog) => error_per_flag!(&mut self.out_of_log_err, "Reversible condition {} got out of log. \
                        Make sure the reversible schedule this condition is in is correctly called in both the forward and backward direction. \
                        It seems that one or more forward schedule calls were missed. \
                        If this condition is in the RevUpdate schedule, this is likely a crate bug.\n{meta:?}", self.name())
                }
            }
        }
    }
    fn apply_deferred(&mut self, _world: &mut World) {}
    fn queue_deferred(&mut self, _world: DeferredWorld) {}
    fn default_system_sets(&self) -> Vec<InternedSystemSet> {
        self.condition.default_system_sets()
    }
    fn update_archetype_component_access(&mut self, world: UnsafeWorldCell) {
        self.condition.update_archetype_component_access(world);
        self.archetype_component_access
            .extend(self.condition.archetype_component_access());
    }
    fn check_change_tick(&mut self, change_tick: Tick) {
        self.condition.check_change_tick(change_tick);
    }
    fn get_last_run(&self) -> Tick {
        self.condition.get_last_run()
    }
    fn set_last_run(&mut self, last_run: Tick) {
        self.condition.set_last_run(last_run);
    }
}

// SAFETY:
// todo
unsafe impl<T: ReadOnlySystem<Out = bool>> ReadOnlySystem for RevCondition<T> {}

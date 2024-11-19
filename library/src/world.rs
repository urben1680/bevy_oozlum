use bevy::ecs::{
    bundle::Bundle,
    component::{Component, ComponentId},
    event::Event,
    observer::{TriggerEvent, TriggerTargets},
    system::IntoObserverSystem,
    world::{DeferredWorld, EntityWorldMut, World},
};

use crate::{
    commands::{buffer_rev_command, RevCommands},
    hook::RevComponentHooks,
    observer::{apply_trigger_event, ObserverLog, RevEvent},
};

pub trait RevWorld {
    fn rev_add_observer<E, B, M>(
        &mut self,
        system: impl IntoObserverSystem<RevEvent<E>, B, M>,
    ) -> EntityWorldMut<'_>
    where
        E: Event + Clone,
        B: Bundle;
    fn rev_trigger(&mut self, event: impl Event + Clone);
    fn rev_trigger_targets(&mut self, event: impl Event + Clone, targets: impl TriggerTargets);
    fn rev_register_component_hooks<T: Component>(&mut self) -> RevComponentHooks;
    fn rev_register_component_hooks_by_id(&mut self, id: ComponentId) -> Option<RevComponentHooks>;
}

impl RevWorld for World {
    fn rev_add_observer<E, B, M>(
        &mut self,
        system: impl IntoObserverSystem<RevEvent<E>, B, M>,
    ) -> EntityWorldMut<'_>
    where
        E: Event + Clone,
        B: Bundle,
    {
        self.init_resource::<ObserverLog<E>>();
        self.add_observer(system)
    }
    fn rev_trigger(&mut self, event: impl Event + Clone) {
        self.rev_trigger_targets(event, ());
    }
    fn rev_trigger_targets(&mut self, event: impl Event + Clone, targets: impl TriggerTargets) {
        let rev_command = apply_trigger_event(TriggerEvent { event, targets }, self);
        if let Some(command) = rev_command {
            buffer_rev_command(&mut self.into(), command);
        }
    }
    fn rev_register_component_hooks<T: Component>(&mut self) -> RevComponentHooks {
        let component = self.register_component::<T>();
        self.rev_register_component_hooks_by_id(component)
            .expect("todo")
    }
    fn rev_register_component_hooks_by_id(&mut self, id: ComponentId) -> Option<RevComponentHooks> {
        self.register_component_hooks_by_id(id)
            .is_some()
            .then_some(RevComponentHooks {
                world: self,
                component: id,
            })
    }
}

pub trait RevDeferredWorld {
    fn rev_trigger(&mut self, event: impl Event + Clone);
    fn rev_trigger_targets(
        &mut self,
        event: impl Event + Clone,
        targets: impl TriggerTargets + Send + 'static,
    );
}

impl<'w> RevDeferredWorld for DeferredWorld<'w> {
    fn rev_trigger(&mut self, event: impl Event + Clone) {
        self.rev_trigger_targets(event, ());
    }
    fn rev_trigger_targets(
        &mut self,
        event: impl Event + Clone,
        targets: impl TriggerTargets + Send + 'static,
    ) {
        self.commands().rev_queue(TriggerEvent { event, targets });
    }
}

pub trait RevEntityWorldMut {
    fn rev_observe<E, B, M>(
        &mut self,
        system: impl IntoObserverSystem<RevEvent<E>, B, M>,
    ) -> &mut Self
    where
        E: Event + Clone,
        B: Bundle;
}

impl RevEntityWorldMut for EntityWorldMut<'_> {
    fn rev_observe<E, B, M>(
        &mut self,
        system: impl IntoObserverSystem<RevEvent<E>, B, M>,
    ) -> &mut Self
    where
        E: Event + Clone,
        B: Bundle,
    {
        let world = unsafe {
            // SAFETY: inserting a resource does not change self's location
            self.world_mut()
        };
        world.init_resource::<ObserverLog<E>>();
        self.observe(system)
    }
}

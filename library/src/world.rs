use bevy::ecs::{
    component::{Component, ComponentId},
    event::Event,
    observer::{TriggerEvent, TriggerTargets},
    world::{DeferredWorld, World},
};

use crate::commands::{buffer_rev_command, hook::RevComponentHooks, observer::apply_trigger_event};

pub trait RevWorld {
    fn rev_trigger(&mut self, event: impl Event + Clone);
    fn rev_trigger_targets(&mut self, event: impl Event + Clone, targets: impl TriggerTargets);
    fn rev_register_component_hooks<T: Component>(&mut self) -> RevComponentHooks;
    fn rev_register_component_hooks_by_id(&mut self, id: ComponentId) -> Option<RevComponentHooks>;
}

pub trait RevDeferredWorld {
    fn rev_trigger(&mut self, event: impl Event + Clone);
    fn rev_trigger_targets(&mut self, event: impl Event + Clone, targets: impl TriggerTargets);
}

impl RevWorld for World {
    fn rev_trigger(&mut self, event: impl Event + Clone) {
        let mut world: DeferredWorld = self.into();
        world.rev_trigger(event);
    }
    fn rev_trigger_targets(&mut self, event: impl Event + Clone, targets: impl TriggerTargets) {
        let mut world: DeferredWorld = self.into();
        world.rev_trigger_targets(event, targets);
    }
    fn rev_register_component_hooks<T: Component>(&mut self) -> RevComponentHooks {
        let component = self.init_component::<T>();
        self.rev_register_component_hooks_by_id(component)
            .expect("todo")
    }
    fn rev_register_component_hooks_by_id(&mut self, id: ComponentId) -> Option<RevComponentHooks> {
        self.register_component_hooks_by_id(id)
            .is_some()
            .then(|| RevComponentHooks {
                world: self,
                component: id,
            })
    }
}

impl<'w> RevDeferredWorld for DeferredWorld<'w> {
    fn rev_trigger(&mut self, event: impl Event + Clone) {
        self.rev_trigger_targets(event, ());
    }
    fn rev_trigger_targets(&mut self, event: impl Event + Clone, targets: impl TriggerTargets) {
        let rev_command = apply_trigger_event(TriggerEvent { event, targets }, self);
        if let Some(command) = rev_command {
            buffer_rev_command(self, command);
        }
    }
}

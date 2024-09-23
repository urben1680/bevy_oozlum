use bevy::{ecs::{component::{ComponentHook, ComponentHooks, ComponentId}, entity::Entity, world::DeferredWorld}, prelude::Resource};

use crate::meta::{Direction, RevMeta};

/*

-- might be possible: apply them at non-log forward (assert!), undo/redo is command logic that does not trigger the hook
-- filter hooks on entities that serve as parking removed components (use extra flag component)
-- filter hooks during log
*/

#[derive(Resource)]
struct ComponentHooksResource {
    on_add: Option<ComponentHook>,
    on_insert: Option<ComponentHook>,
    on_remove: Option<ComponentHook>,
}

struct ComponentHooksInternal {
    on_add: Option<ComponentHook>,
    on_insert: Option<ComponentHook>,
    on_remove: Option<ComponentHook>,
}

pub enum HookDirection {
    Forward {
        log: bool
    },
    Backward,
    /// triggered by actual hook durin log schedule and not the log that undoes hook
    IndeterministicForward,
    /// triggered by actual hook durin log schedule and not the log that undoes hook
    IndeterministicBackward,
    NonReversibleSchedule
}

pub trait RevComponentHooks {
    fn rev_on_add(
        &mut self,
        hook: for<'w> fn(_: Option<Direction>, _: DeferredWorld<'w>, _: Entity, _: ComponentId),
    ) -> &mut Self;
}

impl RevComponentHooks for ComponentHooks {
    fn rev_on_add(
            &mut self,
            hook: for<'w> fn(_: Option<Direction>, _: DeferredWorld<'w>, _: Entity, _: ComponentId),
        ) -> &mut Self {
        /*
        - do not react on removed component container entities
        - run hook if direction is None or Some(Forward(false))
        - the latter issues an initialized command

        

        how to bring the fn into the one below? 
        add it to a resource now
        but ComponentHooks exposes no world, not even deferred to send a command
        so newtype ComponentHooks
         */
        self.on_add(|world, entity, component_id| {
            let direction = world.get_resource::<RevMeta>().and_then(RevMeta::get_direction);
            match direction {
                Some(Direction::Forward { log: false }) => {
                    todo!()
                },
                None => {
                    hook(direction, world, entity, component_id);
                },
                _ => {} 
            }
        });
        self
    }
}
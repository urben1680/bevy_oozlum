use bevy::prelude::{Bundle, Entity, World};

use super::{CommandAction, CommandPanic, ReversibleCommand, ReversibleCommandInitialized};

pub struct SpawnEntity<T: Bundle> {
    data: T,
}

impl<T: Bundle> SpawnEntity<T> {
    pub fn new(data: T) -> Self {
        Self { data }
    }
}

impl<T: Bundle> ReversibleCommand for SpawnEntity<T> {
    fn init(self: Box<Self>, world: &mut World) -> Option<Box<dyn ReversibleCommandInitialized>> {
        Some(Box::new(SpawnEntityInitialized {
            entity: world.spawn().insert_bundle(self.data).id(),
        }))
    }
}

pub struct SpawnEntityInitialized {
    entity: Entity,
}

impl ReversibleCommandInitialized for SpawnEntityInitialized {
    fn action(&mut self, world: &mut World, action: CommandAction) {
        Self::entity(world, action, false, self.entity);
    }
}

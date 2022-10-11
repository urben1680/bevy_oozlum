use crate::DespawnedEntity;
use bevy::prelude::{Bundle, Entity, World};

use super::{ReversibleCommand, ReversibleCommandInitialized};

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
            spawned: true,
            entity: world.spawn().insert_bundle(self.data).id(),
        }))
    }
}

pub struct SpawnEntityInitialized {
    spawned: bool,
    entity: Entity,
}

impl ReversibleCommandInitialized for SpawnEntityInitialized {
    fn undo_redo(&mut self, world: &mut World) {
    }
    fn finalize(self: Box<Self>, world: &mut World) {
    }
}

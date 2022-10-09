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
    fn init(self: Box<Self>, world: &mut World) -> Box<dyn ReversibleCommandInitialized> {
        Box::new(SpawnEntityInitialized {
            entity: world.spawn().insert_bundle(self.data).id(),
        })
    }
}

pub struct SpawnEntityInitialized {
    entity: Entity,
}

impl ReversibleCommandInitialized for SpawnEntityInitialized {
    fn redo(&mut self, world: &mut World) {
        let mut entity = world.entity_mut(self.entity);
        entity.remove::<DespawnedEntity>();
    }
    fn undo(&mut self, world: &mut World) {
        let mut entity = world.entity_mut(self.entity);
        entity.insert(DespawnedEntity);
    }
    fn redo_finalize(self: Box<Self>, _world: &mut World) {}
    fn undo_finalize(self: Box<Self>, world: &mut World) {
        world.despawn(self.entity);
    }
}

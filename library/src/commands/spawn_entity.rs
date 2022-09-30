use bevy::prelude::{Entity, World, Bundle};
use crate::DespawnedEntity;

use super::{ReversibleCommand, ReversibleCommandInitialized};

pub struct SpawnEntity<T: Bundle>{
    data: T
}

impl<T: Bundle> SpawnEntity<T>{
    pub fn new(data: T) -> Self{
        Self{ data }
    }
}

impl<T: Bundle> ReversibleCommand for SpawnEntity<T>{
    type Initialized = SpawnEntityInitialized;
    fn init<Marker>(self, world: &mut World) -> Self::Initialized {
        SpawnEntityInitialized{
            entity: world.spawn().insert_bundle(self.data).id()
        }
    }
}

pub struct SpawnEntityInitialized{
    entity: Entity
}

impl ReversibleCommandInitialized for SpawnEntityInitialized{
    fn redo(&mut self, world: &mut World){
        let mut entity = world.entity_mut(self.entity);
        entity.remove::<DespawnedEntity>();
    }
    fn undo(&mut self, world: &mut World){
        let mut entity = world.entity_mut(self.entity);
        entity.insert(DespawnedEntity);
    }
    fn redo_finalize(&mut self, _world: &mut World){}
    fn undo_finalize(&mut self, world: &mut World) {
        world.despawn(self.entity);
    }
}
use bevy::prelude::{Entity, World, Commands, Bundle};
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
    fn init(self, world: &mut World) -> Self::Initialized {
        SpawnEntityInitialized{
            entity: world.spawn().insert_bundle(self.data).id()
        }
    }
}

pub struct SpawnEntityInitialized{
    entity: Entity
}

impl ReversibleCommandInitialized for SpawnEntityInitialized{
    fn redo(&mut self, commands: &mut Commands){
        let entity = self.entity;
        commands.add(move |world: &mut World|{
            let mut entity = world.entity_mut(entity);
            entity.remove::<DespawnedEntity>();
        });
    }
    fn undo(&mut self, commands: &mut Commands){
        let entity = self.entity;
        commands.add(move |world: &mut World|{
            let mut entity = world.entity_mut(entity);
            entity.insert(DespawnedEntity);
        });
    }
    fn cleanup(&mut self, _commands: &mut Commands){}
}
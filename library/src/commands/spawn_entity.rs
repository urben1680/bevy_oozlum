use crate::DespawnedEntity;
use bevy::prelude::{Bundle, Entity, World};

use super::{ReversibleCommand, ReversibleCommandInitialized, panic_msg};

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
        let mut entity = world.entity_mut(self.entity);
        if self.spawned{
            if entity.remove::<DespawnedEntity>().is_none(){
                panic!("{}", panic_msg::<Self>("undo"));
            }
        } else if entity.contains::<DespawnedEntity>(){
            panic!("{}", panic_msg::<Self>("redo"));
        } else {
            entity.insert(DespawnedEntity);
        }
    }
    fn finalize(self: Box<Self>, world: &mut World) {
        if !self.spawned{
            world.despawn(self.entity);
        }
    }
}

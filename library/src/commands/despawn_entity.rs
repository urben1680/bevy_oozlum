use bevy::prelude::{Entity, World, Commands};
use crate::DespawnedEntity;
use super::{ReversibleCommandErrorHandling, ReversibleCommand, ReversibleCommandInitialized};

#[derive(Debug, Clone, Copy)]
pub enum DespawnEntityError{
    EntityNotFound,
}

pub struct DespawnEntity{
    entity: Entity,
    error: ReversibleCommandErrorHandling<DespawnEntityError>
}

impl DespawnEntity{
    pub fn new_with_error_handling(entity: Entity, error: ReversibleCommandErrorHandling<DespawnEntityError>) -> Self{
        Self { entity, error }
    }
    pub fn new(entity: Entity) -> Self{
        Self::new_with_error_handling(entity, Default::default())
    }
}

impl ReversibleCommand for DespawnEntity{
    type Initialized = DespawnEntityInitialized;
    fn init(self, world: &mut World) -> Self::Initialized {
        if let Some(mut entity_mut) = world.get_entity_mut(self.entity){
            entity_mut.insert(DespawnedEntity);
        } else {
            self.error.error::<()>(&DespawnEntityError::EntityNotFound);
        }
        DespawnEntityInitialized{
            entity: self.entity
        }
    }
}

pub struct DespawnEntityInitialized{
    entity: Entity
}

impl ReversibleCommandInitialized for DespawnEntityInitialized{
    fn redo(&mut self, commands: &mut Commands){
        let entity = self.entity;
        commands.add(move |world: &mut World|{
            let mut entity = world.entity_mut(entity);
            entity.insert(DespawnedEntity);
        });
    }
    fn undo(&mut self, commands: &mut Commands){
        let entity = self.entity;
        commands.add(move |world: &mut World|{
            let mut entity = world.entity_mut(entity);
            entity.remove::<DespawnedEntity>();
        });
    }
    fn cleanup(&mut self, commands: &mut Commands){
        let entity = self.entity;
        commands.add(move |world: &mut World|{
            world.entity_mut(entity).despawn();
        });
    }
}
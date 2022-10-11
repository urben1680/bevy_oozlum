use super::{ReversibleCommand, ReversibleCommandErrorHandling, ReversibleCommandInitialized};
use crate::DespawnedEntity;
use bevy::prelude::{Entity, World};

#[derive(Debug, Clone, Copy)]
pub enum DespawnEntityError {
    EntityNotFound,
}

pub struct DespawnEntity {
    entity: Entity,
    error: ReversibleCommandErrorHandling<DespawnEntityError>,
}

impl DespawnEntity {
    pub fn new_with_error_handling(
        entity: Entity,
        error: ReversibleCommandErrorHandling<DespawnEntityError>,
    ) -> Self {
        Self { entity, error }
    }
    pub fn new(entity: Entity) -> Self {
        Self::new_with_error_handling(entity, Default::default())
    }
}

impl ReversibleCommand for DespawnEntity {
    fn init(self: Box<Self>, world: &mut World) -> Option<Box<dyn ReversibleCommandInitialized>> {
        if let Some(mut entity_mut) = world.get_entity_mut(self.entity) {
            entity_mut.insert(DespawnedEntity);
            Some(Box::new(DespawnEntityInitialized {
                despawned: true,
                entity: self.entity,
            }))
        } else {
            self.error
                .error::<Entity>(&DespawnEntityError::EntityNotFound);
            None
        }
    }
}

pub struct DespawnEntityInitialized {
    despawned: bool,
    entity: Entity,
}

impl ReversibleCommandInitialized for DespawnEntityInitialized {
    fn undo_redo(&mut self, world: &mut World) {
        let mut entity = world.entity_mut(self.entity);
        if self.despawned{
            if entity.remove::<DespawnedEntity>().is_none(){
                
            }
        } else if entity.contains::<DespawnEntity>() {

        } else {
            entity.insert(DespawnedEntity);
        }
        self.despawned = !self.despawned;
    }
    fn finalize(self: Box<Self>, world: &mut World) {
        if self.despawned{
            world.entity_mut(self.entity).despawn();
        }
    }
}

use super::{
    CommandAction, PresetFunctions, ReversibleCommand, ReversibleCommandErrorHandling,
    ReversibleCommandInitialized,
};
use crate::DespawnedEntity;
use bevy::prelude::{Entity, World};

#[derive(Debug, Clone, Copy)]
pub enum DespawnEntityError {
    EntityNotFound,
    EntityDespawned,
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
        if let Some(mut entity) = world.get_entity_mut(self.entity) {
            if entity.contains::<DespawnedEntity>() {
                self.error
                    .error::<Self>(&DespawnEntityError::EntityDespawned);
                None
            } else {
                entity.insert::<DespawnedEntity>(DespawnedEntity);
                Some(Box::new(DespawnEntityInitialized {
                    entity: self.entity,
                }))
            }
        } else {
            self.error
                .error::<Self>(&DespawnEntityError::EntityNotFound);
            None
        }
    }
}

pub struct DespawnEntityInitialized {
    entity: Entity,
}

impl ReversibleCommandInitialized for DespawnEntityInitialized {
    fn action(&mut self, world: &mut World, action: CommandAction) {
        Self::entity(world, action, true, self.entity);
    }
}

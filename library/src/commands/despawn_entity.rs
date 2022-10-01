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
    type Initialized = DespawnEntityInitialized;
    fn init<Marker>(self, world: &mut World) -> Self::Initialized {
        if let Some(mut entity_mut) = world.get_entity_mut(self.entity) {
            entity_mut.insert(DespawnedEntity);
        } else {
            self.error
                .error::<Entity, Marker>(&DespawnEntityError::EntityNotFound);
        }
        DespawnEntityInitialized {
            entity: self.entity,
        }
    }
}

pub struct DespawnEntityInitialized {
    entity: Entity,
}

impl ReversibleCommandInitialized for DespawnEntityInitialized {
    fn redo(&mut self, world: &mut World) {
        let mut entity = world.entity_mut(self.entity);
        entity.insert(DespawnedEntity);
    }
    fn undo(&mut self, world: &mut World) {
        let mut entity = world.entity_mut(self.entity);
        entity.remove::<DespawnedEntity>();
    }
    fn redo_finalize(&mut self, world: &mut World) {
        world.entity_mut(self.entity).despawn();
    }
    fn undo_finalize(&mut self, world: &mut World) {}
}

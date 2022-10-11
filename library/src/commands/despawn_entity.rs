use super::{ReversibleCommand, ReversibleCommandErrorHandling, ReversibleCommandInitialized, CommandPanic};
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
            if entity.contains::<DespawnedEntity>(){
                self.error.error::<Self>(&DespawnEntityError::EntityDespawned);
                None
            }
            else {
                entity.insert::<DespawnedEntity>(DespawnedEntity);
                Some(Box::new(DespawnEntityInitialized{
                    despawned: true,
                    entity: self.entity
                }))
            }
        } else {
            self.error.error::<Self>(&DespawnEntityError::EntityNotFound);
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
        if let Some(mut entity) = world.get_entity_mut(self.entity){
            if self.despawned{
                if entity.remove::<DespawnedEntity>().is_none(){
                    self.panic("respawn failed, entity is not marked as despawned");
                }
            }
            else {
                if entity.contains::<DespawnedEntity>(){
                    self.panic("despawn failed, entity is already marked as despawned");
                }
                else {
                    entity.insert(DespawnedEntity);
                }
            }
        }
        else if self.despawned{
            self.panic("respawn failed, entity not found");
        }
        else {
            self.panic("despawn failed, entity not found");
        }
        self.despawned = !self.despawned;
    }
    fn finalize(self: Box<Self>, world: &mut World) {
        if let Some(mut entity) = world.get_entity_mut(self.entity){
            if self.despawned{
                if !entity.contains::<DespawnedEntity>(){
                    self.panic("finalize despawn failed, entity is not marked as despawned");
                }
                else {
                    entity.despawn();
                }
            }
            else {
                if entity.contains::<DespawnedEntity>(){
                    self.panic("finalize respawn failed, entity is marked as despawned");
                }
            }
        }
        else if self.despawned{
            self.panic("finalize despawn failed, entity not found");
        }
        else {
            self.panic("finalize respawn failed, entity not found");
        }
    }
}

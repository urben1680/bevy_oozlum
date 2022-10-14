use super::{
    CommandAction, PresetFunctions, ReversibleCommand, ReversibleCommandErrorHandling,
    ReversibleCommandInitialized,
};
use crate::{Despawned, DespawnedEntity};
use bevy::prelude::{Component, Entity, World};
use std::marker::PhantomData;

#[derive(Debug, Clone, Copy)]
pub enum DespawnComponentError {
    EntityNotFound,
    EntityDespawned,
    ComponentNotFound,
    AlreadyDespawed,
}

pub struct DespawnComponent<T: Component> {
    entity: Entity,
    error: ReversibleCommandErrorHandling<DespawnComponentError>,
    p: PhantomData<T>,
}

impl<T: Component> DespawnComponent<T> {
    pub fn new_with_error_handling(
        entity: Entity,
        error: ReversibleCommandErrorHandling<DespawnComponentError>,
    ) -> Self {
        Self {
            entity,
            error,
            p: PhantomData,
        }
    }
    pub fn new(entity: Entity) -> Self {
        Self::new_with_error_handling(entity, Default::default())
    }
}

impl<T: Component> ReversibleCommand for DespawnComponent<T> {
    fn init(self: Box<Self>, world: &mut World) -> Option<Box<dyn ReversibleCommandInitialized>> {
        if let Some(mut entity) = world.get_entity_mut(self.entity) {
            if entity.contains::<DespawnedEntity>() {
                self.error
                    .error::<Self>(&DespawnComponentError::EntityDespawned);
                None
            } else if entity.contains::<Despawned<T>>() {
                self.error
                    .error::<Self>(&DespawnComponentError::AlreadyDespawed);
                None
            } else if let Some(data) = entity.remove::<T>() {
                entity.insert(Despawned(data));
                Some(Box::new(DespawnComponentInitialized {
                    entity: self.entity,
                    p: PhantomData::<T>,
                }))
            } else {
                self.error
                    .error::<Self>(&DespawnComponentError::ComponentNotFound);
                None
            }
        } else {
            self.error
                .error::<Self>(&DespawnComponentError::EntityNotFound);
            None
        }
    }
}

pub struct DespawnComponentInitialized<T: Component> {
    entity: Entity,
    p: PhantomData<T>,
}

impl<T: Component> ReversibleCommandInitialized for DespawnComponentInitialized<T> {
    fn undo(&mut self, world: &mut World) {
        Self::component::<T>(world, CommandAction::Undo, true, self.entity);
    }
    fn redo(&mut self, world: &mut World) {
        Self::component::<T>(world, CommandAction::Redo, true, self.entity);        
    }
    fn redo_finalize(self: Box<Self>, world: &mut World) {
        Self::component::<T>(world, CommandAction::RedoFinalize, true, self.entity);        
    }
    fn undo_finalize(self: Box<Self>, world: &mut World) {
        Self::component::<T>(world, CommandAction::UndoFinalize, true, self.entity);        
    }
}

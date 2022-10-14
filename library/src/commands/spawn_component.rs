use crate::{Despawned, DespawnedEntity};

use super::{
    CommandAction, PresetFunctions, ReversibleCommand, ReversibleCommandErrorHandling,
    ReversibleCommandInitialized,
};
use bevy::prelude::{Component, Entity, World};
use std::marker::PhantomData;

#[derive(Debug, Clone, Copy)]
pub enum SpawnComponentError {
    EntityNotFound,
    EntityDespawned,
    AlreadySpawned,
    MarkedDespawned,
}

pub struct SpawnComponent<T: Component> {
    data: T,
    entity: Entity,
    error: ReversibleCommandErrorHandling<SpawnComponentError>,
}

impl<T: Component> SpawnComponent<T> {
    pub fn new_with_error_handling(
        data: T,
        entity: Entity,
        error: ReversibleCommandErrorHandling<SpawnComponentError>,
    ) -> Self {
        Self {
            data,
            entity,
            error,
        }
    }
    pub fn new(data: T, entity: Entity) -> Self {
        Self::new_with_error_handling(data, entity, Default::default())
    }
}

impl<T: Component> ReversibleCommand for SpawnComponent<T> {
    fn init(self: Box<Self>, world: &mut World) -> Option<Box<dyn ReversibleCommandInitialized>> {
        if let Some(mut entity) = world.get_entity_mut(self.entity) {
            if entity.contains::<DespawnedEntity>() {
                self.error.error::<T>(&SpawnComponentError::EntityDespawned);
                None
            } else if entity.contains::<T>() {
                self.error.error::<T>(&SpawnComponentError::AlreadySpawned);
                None
            } else if entity.contains::<Despawned<T>>() {
                self.error.error::<T>(&SpawnComponentError::MarkedDespawned);
                None
            } else {
                entity.insert(self.data);
                Some(Box::new(SpawnComponentInitialized {
                    p: PhantomData::<T>,
                    entity: self.entity,
                }))
            }
        } else {
            self.error.error::<T>(&SpawnComponentError::EntityNotFound);
            None
        }
    }
}

pub struct SpawnComponentInitialized<T: Component> {
    p: PhantomData<T>,
    entity: Entity,
}

impl<T: Component> ReversibleCommandInitialized for SpawnComponentInitialized<T> {
    fn undo(&mut self, world: &mut World) {
        Self::component::<T>(world, CommandAction::Undo, false, self.entity);
    }
    fn redo(&mut self, world: &mut World) {
        Self::component::<T>(world, CommandAction::Redo, false, self.entity);
    }
    fn redo_finalize(self: Box<Self>, world: &mut World) {
        Self::component::<T>(world, CommandAction::RedoFinalize, false, self.entity);
    }
    fn undo_finalize(self: Box<Self>, world: &mut World) {
        Self::component::<T>(world, CommandAction::UndoFinalize, false, self.entity);
    }
}

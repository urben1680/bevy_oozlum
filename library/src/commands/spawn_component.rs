use super::{ReversibleCommand, ReversibleCommandErrorHandling, ReversibleCommandInitialized, panic_msg};
use crate::Despawned;
use bevy::prelude::{Component, Entity, World};
use std::marker::PhantomData;

#[derive(Debug, Clone, Copy)]
pub enum SpawnComponentError {
    EntityNotFound,
    ComponentAlreadyExists,
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
        if let Some(mut entity_mut) = world.get_entity_mut(self.entity) {
            if !entity_mut.contains::<T>() {
                entity_mut.insert(self.data);
                return Some(Box::new(SpawnComponentInitialized {
                    spawned: true,
                    p: PhantomData::<T>,
                    entity: self.entity,
                }));
            } else {
                self.error
                    .error::<T>(&SpawnComponentError::ComponentAlreadyExists);
            }
        } else {
            self.error.error::<T>(&SpawnComponentError::EntityNotFound);
        }
        None
    }
}

pub struct SpawnComponentInitialized<T: Component> {
    spawned: bool,
    p: PhantomData<T>,
    entity: Entity,
}

impl<T: Component> ReversibleCommandInitialized for SpawnComponentInitialized<T> {
    fn undo_redo(&mut self, world: &mut World) {
        let mut entity = world.entity_mut(self.entity);
        if self.spawned{
            let value = entity.remove::<Despawned<T>>().unwrap_or_else(||panic!("{}", panic_msg::<Self>("undo")));
            entity.insert(value.0);
        } else {
            let value = entity.remove::<T>().unwrap_or_else(||panic!("{}", panic_msg::<Self>("redo")));
            entity.insert(Despawned(value));
        }
    }
    fn finalize(self: Box<Self>, world: &mut World) {
        if !self.spawned{
            let mut entity = world.entity_mut(self.entity);
            entity.remove::<Despawned<T>>();
        }
    }
}

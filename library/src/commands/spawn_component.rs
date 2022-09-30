use std::marker::PhantomData;
use bevy::prelude::{Component, Entity, World};
use crate::Despawned;
use super::{ReversibleCommandErrorHandling, ReversibleCommand, ReversibleCommandInitialized};

#[derive(Debug, Clone, Copy)]
pub enum SpawnComponentError{
    EntityNotFound,
    ComponentAlreadyExists,
}

pub struct SpawnComponent<T: Component>{
    data: T,
    entity: Entity,
    error: ReversibleCommandErrorHandling<SpawnComponentError>
}

impl<T: Component> SpawnComponent<T>{
    pub fn new_with_error_handling(data: T, entity: Entity, error: ReversibleCommandErrorHandling<SpawnComponentError>) -> Self{
        Self { data, entity, error }
    }
    pub fn new(data: T, entity: Entity) -> Self{
        Self::new_with_error_handling(data, entity, Default::default())
    }
}

impl<T: Component> ReversibleCommand for SpawnComponent<T>{
    type Initialized = SpawnComponentInitialized<T>;
    fn init<Marker>(self, world: &mut World) -> Self::Initialized {
        if let Some(mut entity_mut) = world.get_entity_mut(self.entity){
            if !entity_mut.contains::<T>(){
                entity_mut.insert(self.data);
            } else {
                self.error.error::<T, Marker>(&SpawnComponentError::ComponentAlreadyExists);
            }
        } else {
            self.error.error::<T, Marker>(&SpawnComponentError::EntityNotFound);
        }
        SpawnComponentInitialized{
            p: PhantomData,
            entity: self.entity
        }
    }
}

pub struct SpawnComponentInitialized<T: Component>{
    p: PhantomData<T>,
    entity: Entity
}

impl<T: Component> ReversibleCommandInitialized for SpawnComponentInitialized<T>{
    fn redo(&mut self, world: &mut World){
        let mut entity = world.entity_mut(self.entity);
        let value = entity.remove::<Despawned<T>>();
        if let Some(value) = value{
            entity.insert(value.0);
        }
    }
    fn undo(&mut self, world: &mut World){
        let mut entity = world.entity_mut(self.entity);
        let value = entity.remove::<T>();
        if let Some(value) = value{
            entity.insert(Despawned(value));
        }
    }
    fn redo_finalize(&mut self, _world: &mut World){}
    fn undo_finalize(&mut self, world: &mut World) {
        let mut entity = world.entity_mut(self.entity);
        entity.remove::<Despawned<T>>();
    }
}
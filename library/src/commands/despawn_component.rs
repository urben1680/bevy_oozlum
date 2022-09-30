use std::marker::PhantomData;
use bevy::prelude::{Component, Entity, World};
use crate::Despawned;
use super::{ReversibleCommandErrorHandling, ReversibleCommand, ReversibleCommandInitialized};

#[derive(Debug, Clone, Copy)]
pub enum DespawnComponentError{
    EntityNotFound,
    ComponentNotFound,
}

pub struct DespawnComponent<T: Component>{
    entity: Entity,
    error: ReversibleCommandErrorHandling<DespawnComponentError>,
    p: PhantomData<T>
}

impl<T: Component> DespawnComponent<T>{
    pub fn new_with_error_handling(entity: Entity, error: ReversibleCommandErrorHandling<DespawnComponentError>) -> Self{
        Self { entity, error, p: PhantomData }
    }
    pub fn new(entity: Entity) -> Self{
        Self::new_with_error_handling(entity, Default::default())
    }
}

impl<T: Component> ReversibleCommand for DespawnComponent<T>{
    type Initialized = DespawnComponentInitialized<T>;
    fn init<Marker>(self, world: &mut World) -> Self::Initialized {
        if let Some(mut entity_mut) = world.get_entity_mut(self.entity){
            if let Some(value) = entity_mut.remove::<T>(){
                entity_mut.insert(Despawned(value));
            } else {
                self.error.error::<T, Marker>(&DespawnComponentError::ComponentNotFound);
            }
        } else {
            self.error.error::<T, Marker>(&DespawnComponentError::EntityNotFound);
        }
        DespawnComponentInitialized{
            entity: self.entity,
            p: PhantomData
        }
    }
}

pub struct DespawnComponentInitialized<T: Component>{
    entity: Entity,
    p: PhantomData<T>
}

impl<T: Component> ReversibleCommandInitialized for DespawnComponentInitialized<T>{
    fn redo(&mut self, world: &mut World){
        let mut entity = world.entity_mut(self.entity);
        let value = entity.remove::<T>();
        if let Some(value) = value{
            entity.insert(Despawned(value));
        }
    }
    fn undo(&mut self, world: &mut World){
        let mut entity = world.entity_mut(self.entity);
        let value = entity.remove::<Despawned<T>>();
        if let Some(value) = value{
            entity.insert(value.0);
        }
}
    fn redo_finalize(&mut self, world: &mut World){
        let mut entity = world.entity_mut(self.entity);
        entity.remove::<Despawned<T>>();
    }
    fn undo_finalize(&mut self, world: &mut World){}
}
use super::{ReversibleCommand, ReversibleCommandErrorHandling, ReversibleCommandInitialized, CommandPanic};
use crate::{Despawned, DespawnedEntity};
use bevy::prelude::{Component, Entity, World};
use std::marker::PhantomData;

#[derive(Debug, Clone, Copy)]
pub enum DespawnComponentError {
    EntityNotFound,
    EntityDespawned,
    ComponentNotFound,
    PreviouslyDespawed,
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
            if entity.contains::<DespawnedEntity>(){
                self.error.error::<Self>(&DespawnComponentError::EntityDespawned);
                None
            }
            else if entity.contains::<Despawned<T>>(){
                self.error.error::<Self>(&DespawnComponentError::PreviouslyDespawed);
                None
            }
            else if let Some(value) = entity.remove::<T>() {
                entity.insert(Despawned(value));
                Some(Box::new(DespawnComponentInitialized {
                    despawned: true,
                    entity: self.entity,
                    p: PhantomData::<T>,
                }))
            } 
            else {
                self.error.error::<Self>(&DespawnComponentError::ComponentNotFound);
                None
            }
        } else {
            self.error.error::<Self>(&DespawnComponentError::EntityNotFound);
            None
        }
    }
}

pub struct DespawnComponentInitialized<T: Component> {
    despawned: bool,
    entity: Entity,
    p: PhantomData<T>,
}

impl<T: Component> ReversibleCommandInitialized for DespawnComponentInitialized<T> {
    fn undo_redo(&mut self, world: &mut World) {
        if let Some(mut entity) = world.get_entity_mut(self.entity){
            if self.despawned{
                if entity.contains::<DespawnedEntity>(){
                    self.panic("respawned failed, entity itself is marked as despawned");
                }
                else if entity.contains::<Despawned<T>>(){
                    self.panic("respawned failed, entity already contains `T`");
                }
                else if let Some(value) = entity.remove::<Despawned<T>>(){
                    entity.insert(value.0);
                }
                else{
                    self.panic("respawned failed, entity does not contain `Despawned<T>`");
                }
            } else {
                if entity.contains::<DespawnedEntity>(){
                    self.panic("despawned failed, entity itself is marked as despawned");
                }
                else if entity.contains::<Despawned<T>>(){
                    self.panic("despawned failed, entity already contains `Despawned<T>`");
                }
                else if let Some(value) = entity.remove::<T>(){
                    entity.insert(Despawned(value));
                }
                else{
                    self.panic("despawned failed, entity does not contain `T`");
                }
            }
        }
        else if self.despawned{
            self.panic("respawned failed, entity not found");
        }
        else {
            self.panic("despawned failed, entity not found");
        }
        self.despawned = !self.despawned;
    }
    fn finalize(self: Box<Self>, world: &mut World) {
        if let Some(mut entity) = world.get_entity_mut(self.entity){
            if self.despawned{
                if entity.contains::<DespawnedEntity>(){
                    self.panic("finalize despawn failed, entity itself is marked as despawned");
                }
                else if entity.contains::<T>(){
                    self.panic("finalize despawn failed, entity contains `T`");
                }
                else if entity.remove::<Despawned<T>>().is_none(){
                    self.panic("finalize despawn failed, entity does not contain `Despawned<T>`")
                }
            }
            else {
                if entity.contains::<DespawnedEntity>(){
                    self.panic("finalize respawn failed, entity itself is marked as despawned");
                }
                else if entity.contains::<Despawned<T>>(){
                    self.panic("finalize respawn failed, entity contains `Despawned<T>`");
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

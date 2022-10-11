use super::{ReversibleCommand, ReversibleCommandErrorHandling, ReversibleCommandInitialized, panic_msg};
use crate::Despawned;
use bevy::prelude::{Component, Entity, World};
use std::marker::PhantomData;

#[derive(Debug, Clone, Copy)]
pub enum DespawnComponentError {
    EntityNotFound,
    ComponentNotFound,
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
        if let Some(mut entity_mut) = world.get_entity_mut(self.entity) {
            if let Some(value) = entity_mut.remove::<T>() {
                entity_mut.insert(Despawned(value));
                return Some(Box::new(DespawnComponentInitialized {
                    despawned: true,
                    entity: self.entity,
                    p: PhantomData::<T>,
                }));
            } else {
                self.error
                    .error::<T>(&DespawnComponentError::ComponentNotFound);
            }
        } else {
            self.error
                .error::<T>(&DespawnComponentError::EntityNotFound);
        }
        None
    }
}

pub struct DespawnComponentInitialized<T: Component> {
    despawned: bool,
    entity: Entity,
    p: PhantomData<T>,
}

impl<T: Component> ReversibleCommandInitialized for DespawnComponentInitialized<T> {
    fn undo_redo(&mut self, world: &mut World) {
        let mut entity = world.entity_mut(self.entity);
        if self.despawned{
            if entity.contains::<Despawned<T>>(){
                panic!("{}", panic_msg::<Self>("undo check"));
            }
            let value = entity.remove::<Despawned<T>>().unwrap_or_else(||panic!("{}", panic_msg::<Self>("undo remove")));
            entity.insert(value.0);
        } else {
            if entity.contains::<T>(){
                panic!("{}", panic_msg::<Self>("redo check"));
            }
            let value = entity.remove::<T>().unwrap_or_else(||panic!("{}", panic_msg::<Self>("redo remove")));
            entity.insert(Despawned(value));
        }
        self.despawned = !self.despawned;
    }
    fn finalize(self: Box<Self>, world: &mut World) {
        if self.despawned{
            if world.entity_mut(self.entity).remove::<Despawned<T>>().is_none(){
                panic!("{}", panic_msg::<Self>("finalize"));
            }
        }
    }
}

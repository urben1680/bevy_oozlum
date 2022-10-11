use super::{ReversibleCommand, ReversibleCommandErrorHandling, ReversibleCommandInitialized, panic_msg};
use crate::Despawned;
use bevy::{ecs::system::Resource, prelude::World};
use std::{marker::PhantomData};

#[derive(Debug, Clone, Copy)]
pub enum DespawnResourceError {
    ResourceNotFound,
}

#[derive(Default)]
pub struct DespawnResource<T: Resource> {
    error: ReversibleCommandErrorHandling<DespawnResourceError>,
    p: PhantomData<T>,
}

impl<T: Resource> DespawnResource<T> {
    pub fn new_with_error_handling(
        error: ReversibleCommandErrorHandling<DespawnResourceError>,
    ) -> Self {
        Self {
            error,
            p: PhantomData,
        }
    }
}

impl<T: Resource> ReversibleCommand for DespawnResource<T> {
    fn init(self: Box<Self>, world: &mut World) -> Option<Box<dyn ReversibleCommandInitialized>> {
        if let Some(value) = world.remove_resource::<T>() {
            world.insert_resource(Despawned(value));
            Some(Box::new(DespawnResourceInitialized {
                despawned: true,
                p: PhantomData::<T>,
            }))
        } else {
            self.error
                .error::<T>(&DespawnResourceError::ResourceNotFound);
            None
        }
    }
}

pub struct DespawnResourceInitialized<T: Resource> {
    despawned: bool,
    p: PhantomData<T>,
}

impl<T: Resource> ReversibleCommandInitialized for DespawnResourceInitialized<T> {
    fn undo_redo(&mut self, world: &mut World) {
        match self.despawned{
            false if world.contains_resource::<DespawnResource<T>>() =>
            false => 
        }


        if self.despawned{
            let value = world.remove_resource::<Despawned<T>>().unwrap_or_else(||panic!("{}", panic_msg::<Self>("undo")));
            if world.contains_resource::<T>(){

            }
            world.insert_resource(value.0);
        } else {
            let value = world.remove_resource::<T>().unwrap_or_else(||panic!("{}", panic_msg::<Self>("redo")));
            world.insert_resource(Despawned(value));
        }
        self.despawned = !self.despawned;
    }
    fn finalize(self: Box<Self>, world: &mut World) {
        if self.despawned{
            if world.remove_resource::<Despawned<T>>().is_none(){
                panic!("{}", panic_msg::<Self>("finalize"));
            }
        }
    }
}

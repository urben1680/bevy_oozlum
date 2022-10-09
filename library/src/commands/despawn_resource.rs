use super::{ReversibleCommand, ReversibleCommandErrorHandling, ReversibleCommandInitialized};
use crate::Despawned;
use bevy::{ecs::system::Resource, prelude::World};
use std::marker::PhantomData;

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
    fn init(self: Box<Self>, world: &mut World) -> Box<dyn ReversibleCommandInitialized> {
        if let Some(value) = world.remove_resource::<T>() {
            world.insert_resource(Despawned(value));
        } else {
            self.error
                .error::<T>(&DespawnResourceError::ResourceNotFound);
        }
        Box::new(DespawnResourceInitialized {
            p: PhantomData::<T>,
        })
    }
}

pub struct DespawnResourceInitialized<T: Resource> {
    p: PhantomData<T>,
}

impl<T: Resource> ReversibleCommandInitialized for DespawnResourceInitialized<T> {
    fn redo(&mut self, world: &mut World) {
        let value = world.remove_resource::<T>();
        if let Some(value) = value {
            world.insert_resource(Despawned(value));
        }
    }
    fn undo(&mut self, world: &mut World) {
        let value = world.remove_resource::<Despawned<T>>();
        if let Some(value) = value {
            world.insert_resource(value.0);
        }
    }
    fn redo_finalize(self: Box<Self>, world: &mut World) {
        world.remove_resource::<Despawned<T>>();
    }
    fn undo_finalize(self: Box<Self>, _world: &mut World) {}
}

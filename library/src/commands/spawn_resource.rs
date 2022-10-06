use super::{ReversibleCommand, ReversibleCommandErrorHandling, ReversibleCommandInitialized};
use crate::Despawned;
use bevy::{ecs::system::Resource, prelude::World};
use std::marker::PhantomData;

#[derive(Debug, Clone, Copy)]
pub enum SpawnResourceError {
    ResourceAlreadyExists,
}

pub struct SpawnResource<T: Resource> {
    data: T,
    error: ReversibleCommandErrorHandling<SpawnResourceError>,
}

impl<T: Resource> SpawnResource<T> {
    pub fn new_with_error_handling(
        data: T,
        error: ReversibleCommandErrorHandling<SpawnResourceError>,
    ) -> Self {
        Self { data, error }
    }
    pub fn new(data: T) -> Self {
        Self::new_with_error_handling(data, Default::default())
    }
}

impl<T: Resource> ReversibleCommand for SpawnResource<T> {
    fn init(self, world: &mut World) -> Box<dyn ReversibleCommandInitialized> {
        if !world.contains_resource::<T>() {
            world.insert_resource(self.data);
        } else {
            self.error
                .error::<T>(&SpawnResourceError::ResourceAlreadyExists);
        }
        Box::new(SpawnResourceInitialized { p: PhantomData::<T> })
    }
}

pub struct SpawnResourceInitialized<T: Resource> {
    p: PhantomData<T>,
}

impl<T: Resource> ReversibleCommandInitialized for SpawnResourceInitialized<T> {
    fn redo(&mut self, world: &mut World) {
        let value = world.remove_resource::<Despawned<T>>();
        if let Some(value) = value {
            world.insert_resource(value.0);
        }
    }
    fn undo(&mut self, world: &mut World) {
        let value = world.remove_resource::<T>();
        if let Some(value) = value {
            world.insert_resource(Despawned(value));
        }
    }
    fn redo_finalize(&mut self, _world: &mut World) {}
    fn undo_finalize(&mut self, world: &mut World) {
        world.remove_resource::<T>();
    }
}

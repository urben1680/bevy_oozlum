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
    fn init(self: Box<Self>, world: &mut World) -> Option<Box<dyn ReversibleCommandInitialized>> {
        None
    }
}

pub struct SpawnResourceInitialized<T: Resource> {
    p: PhantomData<T>,
}

impl<T: Resource> ReversibleCommandInitialized for SpawnResourceInitialized<T> {
    fn undo_redo(&mut self, world: &mut World) {
    }
    fn finalize(self: Box<Self>, world: &mut World) {
    }
}

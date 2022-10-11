use super::{ReversibleCommand, ReversibleCommandErrorHandling, ReversibleCommandInitialized};
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
        None
    }
}

pub struct DespawnResourceInitialized<T: Resource> {
    despawned: bool,
    p: PhantomData<T>,
}

impl<T: Resource> ReversibleCommandInitialized for DespawnResourceInitialized<T> {
    fn undo_redo(&mut self, world: &mut World) {
    }
    fn finalize(self: Box<Self>, world: &mut World) {
    }
}

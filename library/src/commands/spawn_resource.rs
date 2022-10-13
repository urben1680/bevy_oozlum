use crate::Despawned;

use super::{
    CommandAction, PresetFunctions, ReversibleCommand, ReversibleCommandErrorHandling,
    ReversibleCommandInitialized,
};
use bevy::{ecs::system::Resource, prelude::World};
use std::marker::PhantomData;

#[derive(Debug, Clone, Copy)]
pub enum SpawnResourceError {
    AlreadySpawned,
    MarkedDespawned,
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
        if world.contains_resource::<T>() {
            self.error
                .error::<Self>(&SpawnResourceError::AlreadySpawned);
            None
        } else if world.contains_resource::<Despawned<T>>() {
            self.error
                .error::<Self>(&SpawnResourceError::MarkedDespawned);
            None
        } else {
            world.insert_resource(self.data);
            Some(Box::new(SpawnResourceInitialized::<T> { p: PhantomData }))
        }
    }
}

pub struct SpawnResourceInitialized<T: Resource> {
    p: PhantomData<T>,
}

impl<T: Resource> ReversibleCommandInitialized for SpawnResourceInitialized<T> {
    fn action(&mut self, world: &mut World, action: CommandAction) {
        Self::resource::<T>(world, action, false);
    }
}

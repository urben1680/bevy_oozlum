use crate::Despawned;

use super::{
    CommandAction, CommandPanic, ReversibleCommand, ReversibleCommandErrorHandling,
    ReversibleCommandInitialized,
};
use bevy::{ecs::system::Resource, prelude::World};
use std::marker::PhantomData;

#[derive(Debug, Clone, Copy)]
pub enum DespawnResourceError {
    NotFound,
    AlreadyDespawed,
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
        if world.contains_resource::<Despawned<T>>() {
            self.error
                .error::<Self>(&DespawnResourceError::AlreadyDespawed);
            None
        } else if let Some(data) = world.remove_resource::<T>() {
            world.insert_resource(Despawned(data));
            Some(Box::new(DespawnResourceInitialized::<T> { p: PhantomData }))
        } else {
            self.error.error::<Self>(&DespawnResourceError::NotFound);
            None
        }
    }
}

pub struct DespawnResourceInitialized<T: Resource> {
    p: PhantomData<T>,
}

impl<T: Resource> ReversibleCommandInitialized for DespawnResourceInitialized<T> {
    fn action(&mut self, world: &mut World, action: CommandAction) {
        Self::resource::<T>(world, action, true);
    }
}

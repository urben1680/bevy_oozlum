use std::marker::PhantomData;
use bevy::{prelude::{World, Commands}, ecs::system::Resource};
use crate::Despawned;
use super::{ReversibleCommandErrorHandling, ReversibleCommand, ReversibleCommandInitialized};

#[derive(Debug, Clone, Copy)]
pub enum DespawnResourceError{
    ResourceNotFound,
}

#[derive(Default)]
pub struct DespawnResource<T: Resource>{
    error: ReversibleCommandErrorHandling<DespawnResourceError>,
    p: PhantomData<T>
}

impl<T: Resource> DespawnResource<T>{
    pub fn new_with_error_handling(error: ReversibleCommandErrorHandling<DespawnResourceError>) -> Self{
        Self{ error, p: PhantomData }
    }
}

impl<T: Resource> ReversibleCommand for DespawnResource<T>{
    type Initialized = DespawnResourceInitialized<T>;
    fn init(self, world: &mut World) -> Self::Initialized{
        if let Some(value) = world.remove_resource::<T>(){
            world.insert_resource(Despawned(value));
        } else {
            self.error.error::<T>(&DespawnResourceError::ResourceNotFound);
        }
        DespawnResourceInitialized{
            p: PhantomData
        }
    }
}

pub struct DespawnResourceInitialized<T: Resource>{
    p: PhantomData<T>
}

impl<T: Resource> ReversibleCommandInitialized for DespawnResourceInitialized<T>{
    fn redo(&mut self, commands: &mut Commands){
        commands.add(|world: &mut World|{
            let value = world.remove_resource::<T>();
            if let Some(value) = value{
                world.insert_resource(Despawned(value));
            }
        });
    }
    fn undo(&mut self, commands: &mut Commands){
        commands.add(|world: &mut World|{
            let value = world.remove_resource::<Despawned<T>>();
            if let Some(value) = value{
                world.insert_resource(value.0);
            }
        });
    }
    fn cleanup(&mut self, commands: &mut Commands){
        commands.add(|world: &mut World|{
            world.remove_resource::<Despawned<T>>();
        });
    }
}
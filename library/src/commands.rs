use std::{marker::PhantomData, fmt::Debug, any::type_name, mem::{size_of, ManuallyDrop, MaybeUninit}};

use bevy::{ecs::system::Resource, prelude::{Commands, Entity, World, Component, Bundle}, log::{error, warn, info}};

use crate::{DespawnedEntity, Despawned};

use super::controller::Controller;

pub trait ReversibleCommand: Resource{
    /// Initialize by mutating the world, has to include actions that occur in the `redo` method
    fn init(&mut self, world: &mut World);
    /// Deploy commands to redo the related actions
    fn redo(&mut self, commands: &mut Commands);
    /// Deploy commands to undo the related actions
    fn undo(&mut self, commands: &mut Commands);
    /// Cleanup commands like despawning buffers before Self is dropped
    fn cleanup(&mut self, commands: &mut Commands);
}

/// Some LogCommands contain data that is moved on initializing.
/// To keep the size of the trait object low past that, the value might be boxed.
trait BoxOrTiny: Send + Sync + 'static + Sized{
    type Inner: Send + Sync;
    fn into_inner(self) -> Self::Inner;
}

trait BoxIsSmaller: Sized{
    const BOX_IS_SMALLER: bool = size_of::<Self>() > size_of::<Box<Self>>();
}

impl<T> BoxIsSmaller for T{}

struct Tiny<T>(T);

impl<T: Send + Sync + 'static> BoxOrTiny for Tiny<T>{
    type Inner = T;
    fn into_inner(self) -> Self::Inner{
        self.0
    }
}

impl<T: Send + Sync + 'static> BoxOrTiny for Box<T>{
    type Inner = T;
    fn into_inner(self) -> Self::Inner{
        *self
    }
}

pub struct SpawnComponent;

struct SpawnComponentVariant<T: Component, Wrapped: BoxOrTiny<Inner = T>>{
    data: ManuallyDrop<Wrapped>,
    entity: Entity,
    error: ErrorOption<SpawnComponentError>
}

impl SpawnComponent{
    pub fn new_box<T: Component>(entity: Entity, data: T, error: ErrorOption<SpawnComponentError>) -> Box<dyn ReversibleCommand>{
        if T::BOX_IS_SMALLER{
            Box::new(SpawnComponentVariant{
                data: ManuallyDrop::new(Box::new(data)),
                entity,
                error
            })
        } else {
            Box::new(SpawnComponentVariant{
                data: ManuallyDrop::new(Tiny(data)),
                entity,
                error
            })
        }
    }
}

impl<T: Component, Wrapped: BoxOrTiny<Inner = T>> ReversibleCommand for SpawnComponentVariant<T, Wrapped>{
    fn init(&mut self, world: &mut World){
        let value = unsafe{
            //SAFETY: `init` is only called once in the `LogCommands::add` method
            ManuallyDrop::take(&mut self.data)
        }.into_inner();
        if let Some(mut entity_mut) = world.get_entity_mut(self.entity){
            if !entity_mut.contains::<T>(){
                entity_mut.insert(value);
            } else {
                self.error.error::<T>(SpawnComponentError::ComponentAlreadyExists);
            }
        } else {
            self.error.error::<T>(SpawnComponentError::EntityNotFound);
        }
    }
    fn redo(&mut self, commands: &mut Commands){
        let entity = self.entity;
        commands.add(move |world: &mut World|{
            let mut entity = world.entity_mut(entity);
            let value = entity.remove::<Despawned<T>>();
            if let Some(value) = value{
                entity.insert(value.0);
            }
        });
    }
    fn undo(&mut self, commands: &mut Commands){
        let entity = self.entity;
        commands.add(move |world: &mut World|{
            let mut entity = world.entity_mut(entity);
            let value = entity.remove::<T>();
            if let Some(value) = value{
                entity.insert(Despawned(value));
            }
        });
    }
    fn cleanup(&mut self, _commands: &mut Commands){}
}

pub struct DespawnComponent<T: Component>{
    entity: Entity,
    error: ErrorOption<DespawnComponentError>,
    p: PhantomData<T>
}

impl<T: Component> DespawnComponent<T>{
    pub fn new_box(entity: Entity, error: ErrorOption<DespawnComponentError>) -> Box<dyn ReversibleCommand>{
        Box::new(Self{
            entity,
            error,
            p: PhantomData
        })
    }
}

impl<T: Component> ReversibleCommand for DespawnComponent<T>{
    fn init(&mut self, world: &mut World){
        if let Some(mut entity_mut) = world.get_entity_mut(self.entity){
            if let Some(value) = entity_mut.remove::<T>(){
                entity_mut.insert(Despawned(value));
            } else {
                self.error.error::<T>(DespawnComponentError::ComponentNotFound);
            }
        } else {
            self.error.error::<T>(DespawnComponentError::EntityNotFound);
        }
    }
    fn redo(&mut self, commands: &mut Commands){
        let entity = self.entity;
        commands.add(move |world: &mut World|{
            let mut entity = world.entity_mut(entity);
            let value = entity.remove::<T>();
            if let Some(value) = value{
                entity.insert(Despawned(value));
            }
        });
    }
    fn undo(&mut self, commands: &mut Commands){
        let entity = self.entity;
        commands.add(move |world: &mut World|{
            let mut entity = world.entity_mut(entity);
            let value = entity.remove::<Despawned<T>>();
            if let Some(value) = value{
                entity.insert(value.0);
            }
        });
    }
    fn cleanup(&mut self, commands: &mut Commands){
        let entity = self.entity;
        commands.add(move |world: &mut World|{
            let mut entity = world.entity_mut(entity);
            entity.remove::<Despawned<T>>();
        });
    }
}

pub struct SpawnResource;

struct SpawnResourceVariant<T: Resource, Wrapped: BoxOrTiny<Inner = T>>{
    data: ManuallyDrop<Wrapped>,
    error: ErrorOption<SpawnResourceError>
}

impl SpawnResource{
    pub fn new_box<T: Resource>(data: T, error: ErrorOption<SpawnResourceError>) -> Box<dyn ReversibleCommand>{
        if T::BOX_IS_SMALLER{
            Box::new(SpawnResourceVariant{
                data: ManuallyDrop::new(Box::new(data)),
                error
            })
        } else {
            Box::new(SpawnResourceVariant{
                data: ManuallyDrop::new(Tiny(data)),
                error
            })
        }
    }
}

impl<Inner: Resource, T: BoxOrTiny<Inner = Inner>> ReversibleCommand for SpawnResourceVariant<Inner, T>{
    fn init(&mut self, world: &mut World){
        let value = unsafe{
            //SAFETY: `init` is only called once in the `LogCommands::add` method
            ManuallyDrop::take(&mut self.data)
        }.into_inner();
        if !world.contains_resource::<Inner>(){
            world.insert_resource(value);
        } else {
            self.error.error::<Inner>(SpawnResourceError::ResourceAlreadyExists);
        }
    }
    fn redo(&mut self, commands: &mut Commands){
        commands.add(|world: &mut World|{
            let value = world.remove_resource::<Despawned<Inner>>();
            if let Some(value) = value{
                world.insert_resource(value.0);
            }
        });
    }
    fn undo(&mut self, commands: &mut Commands){
        commands.add(|world: &mut World|{
            let value = world.remove_resource::<Inner>();
            if let Some(value) = value{
                world.insert_resource(Despawned(value));
            }
        });
    }
    fn cleanup(&mut self, _commands: &mut Commands){}
}

pub struct DespawnResource<T: Resource>{
    error: ErrorOption<DespawnResourceError>,
    p: PhantomData<T>
}

impl<T: Resource> DespawnResource<T>{
    pub fn new_box(error: ErrorOption<DespawnResourceError>) -> Box<dyn ReversibleCommand>{
        Box::new(Self{
            error,
            p: PhantomData
        })
    }
}

impl<T: Resource> ReversibleCommand for DespawnResource<T>{
    fn init(&mut self, world: &mut World){
        if let Some(value) = world.remove_resource::<T>(){
            world.insert_resource(Despawned(value));
        } else {
            self.error.error::<T>(DespawnResourceError::ResourceNotFound);
        }
    }
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

pub struct SpawnEntity;

struct SpawnEntityVariant<T: Bundle, Wrapped: BoxOrTiny<Inner = T>>{
    data: ManuallyDrop<Wrapped>,
    entity: MaybeUninit<Entity>
}

impl SpawnEntity{
    pub fn new_box<T: Bundle>(data: T) -> Box<dyn ReversibleCommand>{
        if T::BOX_IS_SMALLER{
            Box::new(SpawnEntityVariant{
                data: ManuallyDrop::new(Box::new(data)),
                entity: MaybeUninit::uninit()
            })
        } else {
            Box::new(SpawnEntityVariant{
                data: ManuallyDrop::new(Tiny(data)),
                entity: MaybeUninit::uninit()
            })
        }
    }
}

impl<T: Bundle, Wrapped: BoxOrTiny<Inner = T>> ReversibleCommand for SpawnEntityVariant<T, Wrapped>{
    fn init(&mut self, world: &mut World){
        let value = unsafe{
            //SAFETY: method `Self::init` is only called once in the `LogCommands::add` method
            ManuallyDrop::take(&mut self.data)
        }.into_inner();
        self.entity.write(world.spawn().insert_bundle(value).id());
    }
    fn redo(&mut self, commands: &mut Commands){
        let entity = *unsafe { 
            //SAFETY: entity was written at method `Self::init` before
            self.entity.assume_init_ref() 
        };
        commands.add(move |world: &mut World|{
            let mut entity = world.entity_mut(entity);
            entity.remove::<DespawnedEntity>();
        });
    }
    fn undo(&mut self, commands: &mut Commands){
        let entity = *unsafe { 
            //SAFETY: entity was written at method `Self::init` before
            self.entity.assume_init_ref() 
        };
        commands.add(move |world: &mut World|{
            let mut entity = world.entity_mut(entity);
            entity.insert(DespawnedEntity);
        });
    }
    fn cleanup(&mut self, _commands: &mut Commands){
        unsafe{
            // SAFETY: entity was written at method `Self::init` before
            self.entity.assume_init_drop()
        }
    }
}

pub struct DespawnEntity{
    entity: Entity,
    error: ErrorOption<DespawnEntityError>
}

impl DespawnEntity{
    pub fn new_box(entity: Entity, error: ErrorOption<DespawnEntityError>) -> Box<dyn ReversibleCommand>{
        Box::new(Self{
            entity,
            error,
        })
    }
}

impl ReversibleCommand for DespawnEntity{
    fn init(&mut self, world: &mut World){
        if let Some(mut entity_mut) = world.get_entity_mut(self.entity){
            entity_mut.insert(DespawnedEntity);
        } else {
            self.error.error::<()>(DespawnEntityError::EntityNotFound);
        }
    }
    fn redo(&mut self, commands: &mut Commands){
        let entity = self.entity;
        commands.add(move |world: &mut World|{
            let mut entity = world.entity_mut(entity);
            entity.insert(DespawnedEntity);
        });
    }
    fn undo(&mut self, commands: &mut Commands){
        let entity = self.entity;
        commands.add(move |world: &mut World|{
            let mut entity = world.entity_mut(entity);
            entity.remove::<DespawnedEntity>();
        });
    }
    fn cleanup(&mut self, commands: &mut Commands){
        let entity = self.entity;
        commands.add(move |world: &mut World|{
            world.entity_mut(entity).despawn();
        });
    }
}

#[derive(Debug, Clone, Copy)]
pub enum SpawnComponentError{
    EntityNotFound,
    ComponentAlreadyExists,
}

#[derive(Debug, Clone, Copy)]
pub enum DespawnComponentError{
    EntityNotFound,
    ComponentNotFound,
}

#[derive(Debug, Clone, Copy)]
pub enum SpawnResourceError{
    ResourceAlreadyExists,
}

#[derive(Debug, Clone, Copy)]
pub enum DespawnResourceError{
    ResourceNotFound,
}

#[derive(Debug, Clone, Copy)]
pub enum DespawnEntityError{
    EntityNotFound,
}

pub enum ErrorOption<E: Debug>{
    LogError,
    LogWarning,
    LogInfo,
    Custom(fn(E))
}

impl<E: Debug> ErrorOption<E>{
    fn error<T>(&self, error: E){
        match self{
            Self::LogError => error!("LogCommand failed: {error:?}, relevant type: {}", type_name::<T>()),
            Self::LogWarning => warn!("LogCommand failed: {error:?}, relevant type: {}", type_name::<T>()),
            Self::LogInfo => info!("LogCommand failed: {error:?}, relevant type: {}", type_name::<T>()),
            Self::Custom(f) => f(error)
        }
    }
}

pub struct ReversibleCommands<'w, 's>(pub (super) Commands<'w, 's>);

impl<'w, 's> ReversibleCommands<'w, 's>{
    pub fn add(&mut self, mut command: Box<dyn ReversibleCommand>){
        self.0.add(move |world: &mut World|{
            command.init(world);
            world
                .resource_mut::<Controller>()
                .log
                .back_mut()
                .unwrap()
                .push(command);
        })
    }
}
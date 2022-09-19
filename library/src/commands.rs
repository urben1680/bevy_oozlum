use std::{marker::PhantomData, fmt::Debug, any::type_name};

use bevy::{ecs::system::Resource, prelude::{Commands, Entity, World, Component, Bundle}, log::{error, warn, info}};

use crate::{DespawnedEntity, Despawned};

use super::controller::Controller;

pub trait ReversibleCommand: Send + Sync + 'static{
    /// Deploy commands to redo the related actions
    fn redo(&mut self, commands: &mut Commands);
    /// Deploy commands to undo the related actions
    fn undo(&mut self, commands: &mut Commands);
    /// Cleanup commands like despawning buffers before Self is dropped
    fn cleanup(&mut self, commands: &mut Commands);
}

pub trait ReversibleCommandInit: Send + Sync + 'static{
    type Initialized: ReversibleCommand;
    /// Initialize by mutating the world, has to include actions that occur in the `Self::WithoutInitData::redo` method
    fn init(self, world: &mut World) -> Self::Initialized;
}

pub struct SpawnComponent<T: Component>{
    data: T,
    entity: Entity,
    error: ErrorOption<SpawnComponentError>
}

impl<T: Component> SpawnComponent<T>{
    pub fn new_with_error_handling(data: T, entity: Entity, error: ErrorOption<SpawnComponentError>) -> Self{
        Self { data, entity, error }
    }
    pub fn new(data: T, entity: Entity) -> Self{
        Self::new_with_error_handling(data, entity, Default::default())
    }
}

impl<T: Component> ReversibleCommandInit for SpawnComponent<T>{
    type Initialized = SpawnComponentInitialized<T>;
    fn init(self, world: &mut World) -> Self::Initialized {
        if let Some(mut entity_mut) = world.get_entity_mut(self.entity){
            if !entity_mut.contains::<T>(){
                entity_mut.insert(self.data);
            } else {
                self.error.error::<T>(SpawnComponentError::ComponentAlreadyExists);
            }
        } else {
            self.error.error::<T>(SpawnComponentError::EntityNotFound);
        }
        SpawnComponentInitialized{
            p: PhantomData,
            entity: self.entity
        }
    }
}

pub struct SpawnComponentInitialized<T: Component>{
    p: PhantomData<T>,
    entity: Entity
}

impl<T: Component> ReversibleCommand for SpawnComponentInitialized<T>{
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
    pub fn new_with_error_handling(entity: Entity, error: ErrorOption<DespawnComponentError>) -> Self{
        Self { entity, error, p: PhantomData }
    }
    pub fn new(entity: Entity) -> Self{
        Self::new_with_error_handling(entity, Default::default())
    }
}

impl<T: Component> ReversibleCommandInit for DespawnComponent<T>{
    type Initialized = DespawnComponentInitialized<T>;
    fn init(self, world: &mut World) -> Self::Initialized {
        if let Some(mut entity_mut) = world.get_entity_mut(self.entity){
            if let Some(value) = entity_mut.remove::<T>(){
                entity_mut.insert(Despawned(value));
            } else {
                self.error.error::<T>(DespawnComponentError::ComponentNotFound);
            }
        } else {
            self.error.error::<T>(DespawnComponentError::EntityNotFound);
        }
        DespawnComponentInitialized{
            entity: self.entity,
            p: PhantomData
        }
    }
}

pub struct DespawnComponentInitialized<T: Component>{
    entity: Entity,
    p: PhantomData<T>
}

impl<T: Component> ReversibleCommand for DespawnComponentInitialized<T>{
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

pub struct SpawnResource<T: Resource>{
    data: T,
    error: ErrorOption<SpawnResourceError>
}

impl<T: Resource> SpawnResource<T>{
    pub fn new_with_error_handling(data: T, error: ErrorOption<SpawnResourceError>) -> Self{
        Self { data, error }
    }
    pub fn new(data: T) -> Self{
        Self::new_with_error_handling(data, Default::default())
    }
}

impl<T: Resource> ReversibleCommandInit for SpawnResource<T>{
    type Initialized = SpawnResourceInitialized<T>;
    fn init(self, world: &mut World) -> Self::Initialized{
        if !world.contains_resource::<T>(){
            world.insert_resource(self.data);
        } else {
            self.error.error::<T>(SpawnResourceError::ResourceAlreadyExists);
        }
        SpawnResourceInitialized{
            p: PhantomData
        }
    }
}

pub struct SpawnResourceInitialized<T: Resource>{
    p: PhantomData<T>
}

impl<T: Resource> ReversibleCommand for SpawnResourceInitialized<T>{
    fn redo(&mut self, commands: &mut Commands){
        commands.add(|world: &mut World|{
            let value = world.remove_resource::<Despawned<T>>();
            if let Some(value) = value{
                world.insert_resource(value.0);
            }
        });
    }
    fn undo(&mut self, commands: &mut Commands){
        commands.add(|world: &mut World|{
            let value = world.remove_resource::<T>();
            if let Some(value) = value{
                world.insert_resource(Despawned(value));
            }
        });
    }
    fn cleanup(&mut self, _commands: &mut Commands){}
}

#[derive(Default)]
pub struct DespawnResource<T: Resource>{
    error: ErrorOption<DespawnResourceError>,
    p: PhantomData<T>
}

impl<T: Resource> DespawnResource<T>{
    pub fn new_with_error_handling(error: ErrorOption<DespawnResourceError>) -> Self{
        Self{ error, p: PhantomData }
    }
}

impl<T: Resource> ReversibleCommandInit for DespawnResource<T>{
    type Initialized = DespawnResourceInitialized<T>;
    fn init(self, world: &mut World) -> Self::Initialized{
        if let Some(value) = world.remove_resource::<T>(){
            world.insert_resource(Despawned(value));
        } else {
            self.error.error::<T>(DespawnResourceError::ResourceNotFound);
        }
        DespawnResourceInitialized{
            p: PhantomData
        }
    }
}

pub struct DespawnResourceInitialized<T: Resource>{
    p: PhantomData<T>
}

impl<T: Resource> ReversibleCommand for DespawnResourceInitialized<T>{
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

pub struct SpawnEntity<T: Bundle>{
    data: T
}

impl<T: Bundle> SpawnEntity<T>{
    pub fn new(data: T) -> Self{
        Self{ data }
    }
}

impl<T: Bundle> ReversibleCommandInit for SpawnEntity<T>{
    type Initialized = SpawnEntityInitialized;
    fn init(self, world: &mut World) -> Self::Initialized {
        SpawnEntityInitialized{
            entity: world.spawn().insert_bundle(self.data).id()
        }
    }
}

pub struct SpawnEntityInitialized{
    entity: Entity
}

impl ReversibleCommand for SpawnEntityInitialized{
    fn redo(&mut self, commands: &mut Commands){
        let entity = self.entity;
        commands.add(move |world: &mut World|{
            let mut entity = world.entity_mut(entity);
            entity.remove::<DespawnedEntity>();
        });
    }
    fn undo(&mut self, commands: &mut Commands){
        let entity = self.entity;
        commands.add(move |world: &mut World|{
            let mut entity = world.entity_mut(entity);
            entity.insert(DespawnedEntity);
        });
    }
    fn cleanup(&mut self, _commands: &mut Commands){}
}

pub struct DespawnEntity{
    entity: Entity,
    error: ErrorOption<DespawnEntityError>
}

impl DespawnEntity{
    pub fn new_with_error_handling(entity: Entity, error: ErrorOption<DespawnEntityError>) -> Self{
        Self { entity, error }
    }
    pub fn new(entity: Entity) -> Self{
        Self::new_with_error_handling(entity, Default::default())
    }
}

impl ReversibleCommandInit for DespawnEntity{
    type Initialized = DespawnEntityInitialized;
    fn init(self, world: &mut World) -> Self::Initialized {
        if let Some(mut entity_mut) = world.get_entity_mut(self.entity){
            entity_mut.insert(DespawnedEntity);
        } else {
            self.error.error::<()>(DespawnEntityError::EntityNotFound);
        }
        DespawnEntityInitialized{
            entity: self.entity
        }
    }
}

pub struct DespawnEntityInitialized{
    entity: Entity
}

impl ReversibleCommand for DespawnEntityInitialized{
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

impl<E: Debug> Default for ErrorOption<E>{
    fn default() -> Self {
        Self::LogError
    }
}

pub struct ReversibleCommands<'w, 's>(pub (super) Commands<'w, 's>);

impl<'w, 's> ReversibleCommands<'w, 's>{
    pub fn add<T: ReversibleCommandInit>(&mut self, command: T){
        self.0.add(|world: &mut World|{
            let init = command.init(world);
            world
                .resource_mut::<Controller>()
                .next_entry
                .push(Box::new(init));
        })
    }
}
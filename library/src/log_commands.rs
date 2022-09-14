use std::{marker::PhantomData, fmt::Debug, any::type_name};

use bevy::{ecs::{system::Resource, query::WorldQuery}, prelude::{Commands, Entity, World, Component, Bundle, Without}, log::{error, warn, info}};

use super::master_resource::{Master, MasterEntryTrait};

#[derive(WorldQuery)]
pub struct PresentEntity{
    pub entity: Entity,
    filter: Without<DespawnedEntity>
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
    fn error<T>(self, error: E){
        match self{
            Self::LogError => error!("LogCommand failed: {error:?}, relevant type: {}", type_name::<T>()),
            Self::LogWarning => warn!("LogCommand failed: {error:?}, relevant type: {}", type_name::<T>()),
            Self::LogInfo => info!("LogCommand failed: {error:?}, relevant type: {}", type_name::<T>()),
            Self::Custom(f) => f(error)
        }
    }
}

/// `Commands` wrapper to offer commands compatible to reversible systems.
pub struct LogCommands<'w, 's>(pub (super) Commands<'w, 's>);

impl<'w, 's> LogCommands<'w, 's>{
    pub fn spawn_component<T: Component>(&mut self, entity: Entity, value: T, error: ErrorOption<SpawnComponentError>){
        self.0.add(move |world: &mut World|{
            if let Some(mut entity_mut) = world.get_entity_mut(entity){
                if !entity_mut.contains::<T>(){
                    entity_mut.insert(value);
                    let mut master = world.resource_mut::<Master>();
                    master.log.back_mut().unwrap().push(Box::new(ComponentEntry::<true, T>{
                        entity,
                        p: PhantomData
                    }));
                } else {
                    error.error::<T>(SpawnComponentError::ComponentAlreadyExists);
                }
            } else {
                error.error::<T>(SpawnComponentError::EntityNotFound);
            }
        })
    }
    pub fn despawn_component<T: Component>(&mut self, entity: Entity, error: ErrorOption<DespawnComponentError>){
        self.0.add(move |world: &mut World|{
            if let Some(mut entity_mut) = world.get_entity_mut(entity){
                if let Some(value) = entity_mut.remove::<T>(){
                    entity_mut.insert(Despawned(value));
                    let mut master = world.resource_mut::<Master>();
                    master.log.back_mut().unwrap().push(Box::new(ComponentEntry::<false, T>{
                        entity,
                        p: PhantomData
                    }));
                } else {
                    error.error::<T>(DespawnComponentError::ComponentNotFound);
                }
            } else {
                error.error::<T>(DespawnComponentError::EntityNotFound);
            }
        })
    }
    pub fn spawn_resource<T: Resource>(&mut self, value: T, error: ErrorOption<SpawnResourceError>){
        self.0.add(move |world: &mut World|{
            if !world.contains_resource::<T>(){
                world.insert_resource(value);
                let mut master = world.resource_mut::<Master>();
                master.log.back_mut().unwrap().push(Box::new(ResourceEntry::<true, T>{
                    p: PhantomData
                }));
            } else {
                error.error::<T>(SpawnResourceError::ResourceAlreadyExists);
            }
        })
    }
    pub fn despawn_resource<T: Resource>(&mut self, error: ErrorOption<DespawnResourceError>){
        self.0.add(move |world: &mut World|{
            if let Some(value) = world.remove_resource::<T>(){
                world.insert_resource(Despawned(value));
                let mut master = world.resource_mut::<Master>();
                master.log.back_mut().unwrap().push(Box::new(ResourceEntry::<false, T>{
                    p: PhantomData
                }));
            } else {
                error.error::<T>(DespawnResourceError::ResourceNotFound);
            }
        })
    }
    pub fn spawn_entity<T: Bundle>(&mut self, bundle: T){
        self.0.add(move |world: &mut World|{
            let entity = world.spawn().insert_bundle(bundle).id();
            let mut master = world.resource_mut::<Master>();
            master.log.back_mut().unwrap().push(Box::new(EntityEntry::<true>{
                entity
            }));
        })
    }
    pub fn despawn_entity(&mut self, entity: Entity, error: ErrorOption<DespawnEntityError>){
        self.0.add(move |world: &mut World|{
            if let Some(mut entity_mut) = world.get_entity_mut(entity){
                entity_mut.insert(DespawnedEntity);
                let mut master = world.resource_mut::<Master>();
                master.log.back_mut().unwrap().push(Box::new(EntityEntry::<false>{
                    entity
                }));
            } else {
                error.error::<()>(DespawnEntityError::EntityNotFound);
            }
        })
    }
}

struct ComponentEntry<const SPAWNED: bool, T: Component>{
    entity: Entity,
    p: PhantomData<T>
}

impl<const SPAWNED: bool, T: Component> ComponentEntry<SPAWNED, T>{
    fn command<const FORWARD: bool>(&self, commands: &mut Commands){
        let entity = self.entity;
        if SPAWNED == FORWARD{
            commands.add(move |world: &mut World|{
                let mut entity = world.entity_mut(entity);
                let value = entity.remove::<Despawned<T>>();
                if let Some(value) = value{
                    entity.insert(value.0);
                }
            });
        } else {
            commands.add(move |world: &mut World|{
                let mut entity = world.entity_mut(entity);
                let value = entity.remove::<T>();
                if let Some(value) = value{
                    entity.insert(Despawned(value));
                }
            });
        }
    }
}

struct ResourceEntry<const SPAWNED: bool, T: Resource>{
    p: PhantomData<T>
}

impl<const SPAWNED: bool, T: Resource> ResourceEntry<SPAWNED, T>{
    fn command<const FORWARD: bool>(&self, commands: &mut Commands){
        if SPAWNED == FORWARD{
            commands.add(|world: &mut World|{
                let value = world.remove_resource::<Despawned<T>>();
                if let Some(value) = value{
                    world.insert_resource(value.0);
                }
            });
        } else {
            commands.add(|world: &mut World|{
                let value = world.remove_resource::<T>();
                if let Some(value) = value{
                    world.insert_resource(Despawned(value));
                }
            });
        }
    }
}

struct EntityEntry<const SPAWNED: bool>{
    entity: Entity
}

impl<const SPAWNED: bool> EntityEntry<SPAWNED>{
    fn command<const FORWARD: bool>(&self, commands: &mut Commands){
        let entity = self.entity;
        if SPAWNED == FORWARD{
            commands.add(move |world: &mut World|{
                let mut entity = world.entity_mut(entity);
                entity.remove::<DespawnedEntity>();
            });
        } else {
            commands.add(move |world: &mut World|{
                let mut entity = world.entity_mut(entity);
                entity.insert(DespawnedEntity);
            });
        }
    }
}

struct Despawned<T>(T);

impl<T: Component> Component for Despawned<T>{
    type Storage = <T as Component>::Storage;
}

#[derive(Component)]
struct DespawnedEntity;

impl<const SPAWNED: bool, T: Component> MasterEntryTrait for ComponentEntry<SPAWNED, T>{
    fn forward(&self, commands: &mut Commands){
        self.command::<true>(commands);
    }
    fn backward(&self, commands: &mut Commands){
        self.command::<false>(commands);
    }
    fn forget(&self, commands: &mut Commands){
        if SPAWNED{
            return;
        }
        let entity = self.entity;
        commands.add(move |world: &mut World|{
            let mut entity = world.entity_mut(entity);
            entity.remove::<Despawned<T>>();
        });
    }
}

impl<const SPAWNED: bool, T: Resource> MasterEntryTrait for ResourceEntry<SPAWNED, T>{
    fn forward(&self, commands: &mut Commands){
        self.command::<true>(commands);
    }
    fn backward(&self, commands: &mut Commands){
        self.command::<false>(commands);
    }
    fn forget(&self, commands: &mut Commands){
        if SPAWNED{
            return;
        }
        commands.add(|world: &mut World|{
            world.remove_resource::<Despawned<T>>();
        });
    }
}

impl<const SPAWNED: bool> MasterEntryTrait for EntityEntry<SPAWNED>{
    fn forward(&self, commands: &mut Commands){
        self.command::<true>(commands);
    }
    fn backward(&self, commands: &mut Commands){
        self.command::<false>(commands);
    }
    fn forget(&self, commands: &mut Commands){
        if SPAWNED{
            return;
        }
        let entity = self.entity;
        commands.add(move |world: &mut World|{
            world.entity_mut(entity).despawn();
        });
    }
}
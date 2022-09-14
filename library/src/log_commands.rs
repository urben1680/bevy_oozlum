use std::{marker::PhantomData, fmt::Debug, any::type_name, mem::{replace, size_of}};

use bevy::{ecs::{system::Resource, query::WorldQuery}, prelude::{Commands, Entity, World, Component, Bundle, Without}, log::{error, warn, info}};

use super::master_resource::{Master, MasterEntryTrait};

pub struct Tiny<T>(pub T);

impl<T> Tiny<T>{
    fn new(inner: T) -> Self{
        Self(inner)
    }
}

pub trait BoxOrTiny: Send + Sync + 'static{
    type Inner: Send + Sync;
    fn to_inner(self) -> Self::Inner;
}

impl<T: Send + Sync + 'static> BoxOrTiny for Box<T>{
    type Inner = T;
    fn to_inner(self) -> Self::Inner{
        /*
        const _: () = assert!(
            size_of::<Box<T>>() < size_of::<Tiny<T>>, 
            "{} should be put into the `Tiny` wrapper.", type_name::<T>()
        );
        */
        *self
    }
}

impl<T: Send + Sync + 'static> BoxOrTiny for Tiny<T>{
    type Inner = T;
    fn to_inner(self) -> Self::Inner{
        /*
        const _: () = assert!(
            size_of::<Tiny<T>>() <= size_of::<Box<T>>, 
            "{} should be put into a `Box`.", type_name::<T>()
        );
        */
        self.0
    }
}

pub struct SpawnComponent<T: Component, Wrapped: BoxOrTiny<Inner = T>>{
    data: Option<Wrapped>,
    entity: Entity,
    error: ErrorOption<SpawnComponentError>
}

impl<T: Component, Wrapped: BoxOrTiny<Inner = T>> SpawnComponent<T, Wrapped>{
    pub fn new(entity: Entity, data: Wrapped, error: ErrorOption<SpawnComponentError>) -> Self{
        Self{
            data: Some(data),
            entity,
            error
        }
    }
}

impl<T: Component, Wrapped: BoxOrTiny<Inner = T>> MasterEntryTrait for SpawnComponent<T, Wrapped>{
    fn init(&mut self, world: &mut World){
        let value = replace(&mut self.data, None).unwrap().to_inner();
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
    fn forward(&mut self, commands: &mut Commands){
        let entity = self.entity;
        commands.add(move |world: &mut World|{
            let mut entity = world.entity_mut(entity);
            let value = entity.remove::<Despawned<T>>();
            if let Some(value) = value{
                entity.insert(value.0);
            }
        });
    }
    fn backward(&mut self, commands: &mut Commands){
        let entity = self.entity;
        commands.add(move |world: &mut World|{
            let mut entity = world.entity_mut(entity);
            let value = entity.remove::<T>();
            if let Some(value) = value{
                entity.insert(Despawned(value));
            }
        });
    }
    fn forget(&mut self, _commands: &mut Commands){}
}

pub struct DespawnComponent<T: Component>{
    entity: Entity,
    error: ErrorOption<DespawnComponentError>,
    p: PhantomData<T>
}

impl<T: Component> DespawnComponent<T>{
    pub fn new(entity: Entity, error: ErrorOption<DespawnComponentError>) -> Self{
        Self{
            entity,
            error,
            p: PhantomData
        }
    }
}

impl<T: Component> MasterEntryTrait for DespawnComponent<T>{
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
    fn forward(&mut self, commands: &mut Commands){
        let entity = self.entity;
        commands.add(move |world: &mut World|{
            let mut entity = world.entity_mut(entity);
            let value = entity.remove::<T>();
            if let Some(value) = value{
                entity.insert(Despawned(value));
            }
        });
    }
    fn backward(&mut self, commands: &mut Commands){
        let entity = self.entity;
        commands.add(move |world: &mut World|{
            let mut entity = world.entity_mut(entity);
            let value = entity.remove::<Despawned<T>>();
            if let Some(value) = value{
                entity.insert(value.0);
            }
        });
    }
    fn forget(&mut self, commands: &mut Commands){
        let entity = self.entity;
        commands.add(move |world: &mut World|{
            let mut entity = world.entity_mut(entity);
            entity.remove::<Despawned<T>>();
        });
    }
}

pub struct SpawnResource<T: Resource, Wrapped: BoxOrTiny<Inner = T>>{
    data: Option<Wrapped>,
    error: ErrorOption<SpawnResourceError>
}

impl<T: Resource, Wrapped: BoxOrTiny<Inner = T>> SpawnResource<T, Wrapped>{
    pub fn new(data: Wrapped, error: ErrorOption<SpawnResourceError>) -> Self{
        Self{
            data: Some(data),
            error
        }
    }
}

impl<Inner: Resource, T: BoxOrTiny<Inner = Inner>> MasterEntryTrait for SpawnResource<Inner, T>{
    fn init(&mut self, world: &mut World){
        let value = replace(&mut self.data, None).unwrap().to_inner();
        if !world.contains_resource::<Inner>(){
            world.insert_resource(value);
        } else {
            self.error.error::<Inner>(SpawnResourceError::ResourceAlreadyExists);
        }
    }
    fn forward(&mut self, commands: &mut Commands){
        commands.add(|world: &mut World|{
            let value = world.remove_resource::<Despawned<Inner>>();
            if let Some(value) = value{
                world.insert_resource(value.0);
            }
        });
    }
    fn backward(&mut self, commands: &mut Commands){
        commands.add(|world: &mut World|{
            let value = world.remove_resource::<Inner>();
            if let Some(value) = value{
                world.insert_resource(Despawned(value));
            }
        });
    }
    fn forget(&mut self, _commands: &mut Commands){}
}

pub struct DespawnResource<T: Resource>{
    error: ErrorOption<DespawnResourceError>,
    p: PhantomData<T>
}

impl<T: Resource> DespawnResource<T>{
    pub fn new(error: ErrorOption<DespawnResourceError>) -> Self{
        Self{
            error,
            p: PhantomData
        }
    }
}

impl<T: Resource> MasterEntryTrait for DespawnResource<T>{
    fn init(&mut self, world: &mut World){
        if let Some(value) = world.remove_resource::<T>(){
            world.insert_resource(Despawned(value));
        } else {
            self.error.error::<T>(DespawnResourceError::ResourceNotFound);
        }
    }
    fn forward(&mut self, commands: &mut Commands){
        commands.add(|world: &mut World|{
            let value = world.remove_resource::<T>();
            if let Some(value) = value{
                world.insert_resource(Despawned(value));
            }
        });
    }
    fn backward(&mut self, commands: &mut Commands){
        commands.add(|world: &mut World|{
            let value = world.remove_resource::<Despawned<T>>();
            if let Some(value) = value{
                world.insert_resource(value.0);
            }
        });
    }
    fn forget(&mut self, commands: &mut Commands){
        commands.add(|world: &mut World|{
            world.remove_resource::<Despawned<T>>();
        });
    }
}

pub struct SpawnEntity<T: Bundle, Wrapped: BoxOrTiny<Inner = T>>{
    data: Option<Wrapped>,
    entity: Option<Entity>
}

impl<T: Bundle, Wrapped: BoxOrTiny<Inner = T>> SpawnEntity<T, Wrapped>{
    pub fn new(data: Wrapped) -> Self{
        Self{
            data: Some(data),
            entity: None
        }
    }
}

impl<T: Bundle, Wrapped: BoxOrTiny<Inner = T>> MasterEntryTrait for SpawnEntity<T, Wrapped>{
    fn init(&mut self, world: &mut World){
        let value = replace(&mut self.data, None).unwrap().to_inner();
        self.entity = Some(world.spawn().insert_bundle(value).id());
    }
    fn forward(&mut self, commands: &mut Commands){
        let entity = self.entity.unwrap();
        commands.add(move |world: &mut World|{
            let mut entity = world.entity_mut(entity);
            entity.remove::<DespawnedEntity>();
        });
    }
    fn backward(&mut self, commands: &mut Commands){
        let entity = self.entity.unwrap();
        commands.add(move |world: &mut World|{
            let mut entity = world.entity_mut(entity);
            entity.insert(DespawnedEntity);
        });
    }
    fn forget(&mut self, _commands: &mut Commands){}
}

pub struct DespawnEntity{
    entity: Entity,
    error: ErrorOption<DespawnEntityError>
}

impl DespawnEntity{
    pub fn new(entity: Entity, error: ErrorOption<DespawnEntityError>) -> Self{
        Self{
            entity,
            error,
        }
    }
}

impl MasterEntryTrait for DespawnEntity{
    fn init(&mut self, world: &mut World){
        if let Some(mut entity_mut) = world.get_entity_mut(self.entity){
            entity_mut.insert(DespawnedEntity);
        } else {
            self.error.error::<()>(DespawnEntityError::EntityNotFound);
        }
    }
    fn forward(&mut self, commands: &mut Commands){
        let entity = self.entity;
        commands.add(move |world: &mut World|{
            let mut entity = world.entity_mut(entity);
            entity.insert(DespawnedEntity);
        });
    }
    fn backward(&mut self, commands: &mut Commands){
        let entity = self.entity;
        commands.add(move |world: &mut World|{
            let mut entity = world.entity_mut(entity);
            entity.remove::<DespawnedEntity>();
        });
    }
    fn forget(&mut self, commands: &mut Commands){
        let entity = self.entity;
        commands.add(move |world: &mut World|{
            world.entity_mut(entity).despawn();
        });
    }
}

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
    fn error<T>(&self, error: E){
        match self{
            Self::LogError => error!("LogCommand failed: {error:?}, relevant type: {}", type_name::<T>()),
            Self::LogWarning => warn!("LogCommand failed: {error:?}, relevant type: {}", type_name::<T>()),
            Self::LogInfo => info!("LogCommand failed: {error:?}, relevant type: {}", type_name::<T>()),
            Self::Custom(f) => f(error)
        }
    }
}

pub struct LogCommands<'w, 's>(pub (super) Commands<'w, 's>);

impl<'w, 's> LogCommands<'w, 's>{
    pub fn add<T: MasterEntryTrait>(&mut self, mut command: T){
        self.0.add(move |world: &mut World|{
            command.init(world);
            let mut master = world.resource_mut::<Master>();
            master.log.back_mut().unwrap().push(Box::new(command));
        })
    }
}

struct Despawned<T>(T);

impl<T: Component> Component for Despawned<T>{
    type Storage = <T as Component>::Storage;
}

#[derive(Component)]
struct DespawnedEntity;
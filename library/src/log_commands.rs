use std::marker::PhantomData;

use bevy::{ecs::system::Resource, prelude::{Commands, Entity, World, Component, Bundle}};

use super::master::{Master, MasterEntryTrait};

/// `Commands` wrapper to offer commands compatible to the crate goal.
pub struct LogCommands<'w, 's>(pub (super) Commands<'w, 's>);

impl<'w, 's> LogCommands<'w, 's>{
    pub fn spawn_component<T: Component>(&mut self, entity: Entity, value: T){
        self.0.add(move |world: &mut World|{
            world.entity_mut(entity).insert(value);
            let mut master = world.resource_mut::<Master>();
            master.log.back_mut().unwrap().push(Box::new(ComponentEntry::<true, T>{
                entity,
                p: PhantomData
            }));
        })
    }
    pub fn despawn_component<T: Component>(&mut self, entity: Entity){
        self.0.add(move |world: &mut World|{
            let mut master = world.resource_mut::<Master>();
            master.log.back_mut().unwrap().push(Box::new(ComponentEntry::<false, T>{
                entity,
                p: PhantomData
            }));
        })
    }
    pub fn spawn_resource<T: Resource>(&mut self, value: T){
        self.0.add(move |world: &mut World|{
            world.insert_resource(value);
            let mut master = world.resource_mut::<Master>();
            master.log.back_mut().unwrap().push(Box::new(ResourceEntry::<true, T>{
                p: PhantomData
            }));
        })
    }
    pub fn despawn_resource<T: Resource>(&mut self){
        self.0.add(move |world: &mut World|{
            let mut master = world.resource_mut::<Master>();
            master.log.back_mut().unwrap().push(Box::new(ResourceEntry::<false, T>{
                p: PhantomData
            }));
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
    pub fn despawn_entity(&mut self, entity: Entity){
        self.0.add(move |world: &mut World|{
            let mut master = world.resource_mut::<Master>();
            master.log.back_mut().unwrap().push(Box::new(EntityEntry::<false>{
                entity
            }));
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
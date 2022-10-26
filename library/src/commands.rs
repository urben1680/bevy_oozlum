use bevy::{
    ecs::{system::Resource, world::EntityMut},
    log::{error, info, warn},
    prelude::{Component, Entity, World},
};
use std::{any::type_name, fmt::Debug};

mod despawn_component;
mod despawn_entity;
mod despawn_resource;
mod spawn_component;
mod spawn_entity;
mod spawn_resource;

pub use despawn_component::*;
pub use despawn_entity::*;
pub use despawn_resource::*;
pub use spawn_component::*;
pub use spawn_entity::*;
pub use spawn_resource::*;

use crate::{Despawned, DespawnedEntity};

//TODO: Conflicting commands in controller detection? reaction? panic?

/// Trait for reversible commands that are not yet initialized.
pub trait ReversibleCommand: Send + Sync + 'static {
    /// returns `Some` if init was successful or an alternative command could be deployed.
    fn init(self: Box<Self>, world: &mut World) -> Option<Box<dyn ReversibleCommandInitialized>>;
}

/// Trait for reversible commands that are initialized.
pub trait ReversibleCommandInitialized: Send + Sync + 'static {
    /// Redo or undo the command. `Self` needs to track itself if `redo` or `undo`is to be applied. The first call to this function is `undo`.
    fn undo(&mut self, world: &mut World);
    fn redo(&mut self, world: &mut World);
    fn redo_finalize(self: Box<Self>, world: &mut World);
    fn undo_finalize(self: Box<Self>, world: &mut World);
}

#[derive(Debug, Copy, Clone)]
enum CommandAction {
    /// Undo the actions done by the initialization
    Undo,
    /// Redo the actions done by the initialization
    Redo,
    /// Cleanup after the actions done by the initialization have been undone
    UndoFinalize,
    /// Cleanup after the actions done by the initialization have been redone
    RedoFinalize,
}

impl CommandAction {
    /// This action causes cleanups
    pub fn finalize(&self) -> bool {
        matches!(self, Self::UndoFinalize | Self::RedoFinalize)
    }
    /// The command was previously undone before this action was issued.
    pub fn currently_undone(&self) -> bool {
        matches!(self, Self::Redo | Self::UndoFinalize)
    }
}

/// Options for errorhandling of the error type `E` in reversible commands.
///
/// - `LogError` variant uses bevy's `log::error` macro
/// - `LogError` variant uses bevy's `log::warn` macro
/// - `LogError` variant uses bevy's `log::info` macro
///
/// The above variants call their respective macro with these parameters:
///
/// `"LogCommand failed: {error:?} for type {}, issued by reversible system {}", std::any::type_name::<T>(), std::any::type_name::<M>()`
///
/// ...where `error` is `&E` and `T` is an additional type that is relevant for this command, like the type of a resource that is tried to be spawned.
///
/// A custom error handling can be set with the variant `Custom` which calls a `fn(&E)` in error cases.
pub enum ReversibleCommandErrorHandling<E: Debug> {
    LogError,
    LogWarning,
    LogInfo,
    Custom(Box<dyn Fn(&E) + Send + Sync>),
}

impl<E: Debug> ReversibleCommandErrorHandling<E> {
    fn error<T>(&self, error: &E) {
        match self {
            Self::LogError => error!("LogCommand failed: {error:?} for type {}", type_name::<T>()),
            Self::LogWarning => warn!("LogCommand failed: {error:?} for type {}", type_name::<T>()),
            Self::LogInfo => info!("LogCommand failed: {error:?} for type {}", type_name::<T>()),
            Self::Custom(f) => f(error),
        }
    }
}

impl<E: Debug> Default for ReversibleCommandErrorHandling<E> {
    fn default() -> Self {
        Self::LogError
    }
}

trait PresetFunctions {
    fn get_entity<'a>(
        world: &'a mut World,
        entity: Entity,
        action: CommandAction,
    ) -> EntityMut<'a> {
        world
            .get_entity_mut(entity)
            .unwrap_or_else(|| Self::panic(action, "entity was not found"))
    }
    fn entity(world: &mut World, action: CommandAction, undo_spawns: bool, entity: Entity) {
        let mut entity = Self::get_entity(world, entity, action);
        if undo_spawns != action.currently_undone() {
            //entity is currently not marked as despawned
            //unmark entity as despawned or finalize unspawned state
            if entity.remove::<DespawnedEntity>().is_some() {
                if action.finalize() {
                    entity.despawn();
                }
            } else {
                Self::despawned_entity_panic(action, false);
            }
        } else {
            //entity is currently marked as despawned
            //mark entity as despawned or finalize spawned state
            if entity.contains::<DespawnedEntity>() {
                Self::despawned_entity_panic(action, true);
            } else if !action.finalize() {
                entity.insert(DespawnedEntity);
            }
        }
    }
    fn component<T: Component>(
        world: &mut World,
        action: CommandAction,
        undo_spawns: bool,
        entity: Entity,
    ) {
        let mut entity = Self::get_entity(world, entity, action);
        if entity.contains::<DespawnedEntity>() {
            Self::despawned_entity_panic(action, true)
        }
        if undo_spawns != action.currently_undone() {
            //component is currently spawned
            //spawn component or finalize unspawned state
            if entity.contains::<T>() {
                Self::t_panic(action, true);
            } else if let Some(value) = entity.remove::<Despawned<T>>() {
                if !action.finalize() {
                    entity.insert(value.0);
                }
            } else {
                Self::despawned_t_panic(action, false);
            }
        } else {
            //component is currently not spawned
            //despawn component or finalize spawned state
            if entity.contains::<Despawned<T>>() {
                Self::despawned_t_panic(action, true);
            }
            if !action.finalize() {
                if let Some(value) = entity.remove::<T>() {
                    entity.insert(Despawned(value));
                } else {
                    Self::t_panic(action, false);
                }
            }
        }
    }
    fn resource<T: Resource>(world: &mut World, action: CommandAction, undo_spawns: bool) {
        if undo_spawns != action.currently_undone() {
            //resource is currently spawned
            //spawn resource or finalize unspawned state
            if world.contains_resource::<T>() {
                Self::t_panic(action, true);
            } else if let Some(value) = world.remove_resource::<Despawned<T>>() {
                if !action.finalize() {
                    world.insert_resource(value.0);
                }
            } else {
                Self::despawned_t_panic(action, false);
            }
        } else {
            //resource is currently not spawned
            //despawn resource or finalize spawned state
            if world.contains_resource::<Despawned<T>>() {
                Self::despawned_t_panic(action, true);
            }
            if !action.finalize() {
                if let Some(value) = world.remove_resource::<T>() {
                    world.insert_resource(Despawned(value));
                } else {
                    Self::t_panic(action, false);
                }
            }
        }
    }
    fn despawned_entity_panic(action: CommandAction, found: bool) {
        if found {
            Self::panic(action, "`DespawnedEntity` was found")
        } else {
            Self::panic(action, "`DespawnedEntity` was not found")
        }
    }
    fn despawned_t_panic(action: CommandAction, found: bool) {
        if found {
            Self::panic(action, "`Despawned<T>` was found")
        } else {
            Self::panic(action, "`Despawned<T>` was not found")
        }
    }
    fn t_panic(action: CommandAction, found: bool) {
        if found {
            Self::panic(action, "`T` was found")
        } else {
            Self::panic(action, "`T` was not found")
        }
    }
    fn panic<R>(action: CommandAction, s: &'static str) -> R {
        panic!(
            "Reversible command `{}` failed at \"{action:?}\" because \"{s}\", which was not expected.",
            type_name::<Self>()
        );
    }
}

impl<T> PresetFunctions for T {}

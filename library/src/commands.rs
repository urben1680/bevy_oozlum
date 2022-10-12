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

/// Trait for reversible commands that are not yet initialized.
pub trait ReversibleCommand: Send + Sync + 'static {
    //returns `Some` if init was successful.
    fn init(self: Box<Self>, world: &mut World) -> Option<Box<dyn ReversibleCommandInitialized>>;
}

#[derive(Debug, Copy, Clone)]
pub enum CommandAction {
    Undo,
    Redo,
    UndoFinalize,
    RedoFinalize,
}

impl CommandAction {
    fn finalize(&self) -> bool {
        matches!(self, Self::UndoFinalize | Self::RedoFinalize)
    }
}

/// Trait for reversible commands that are initialized.
pub trait ReversibleCommandInitialized: Send + Sync + 'static {
    /// Redo or undo the command. `Self` needs to track itself if `redo` or `undo`is to be applied. The first call to this function is `undo`.
    fn action(&mut self, world: &mut World, action: CommandAction);
}

trait CommandPanic {
    fn get_entity<'a>(
        world: &'a mut World,
        entity: Entity,
        action: CommandAction,
    ) -> EntityMut<'a> {
        match world.get_entity_mut(entity) {
            Some(entity) => entity,
            None => Self::panic(action, "entity not found"),
        }
    }
    fn entity(world: &mut World, action: CommandAction, undo_spawns: bool, entity: Entity) {
        let mut entity = Self::get_entity(world, entity, action);
        if undo_spawns == matches!(action, CommandAction::Undo | CommandAction::RedoFinalize) {
            if entity.remove::<DespawnedEntity>().is_none() {
                Self::despawned_entity(action, false);
            }
        } else {
            if entity.contains::<DespawnedEntity>() {
                entity.insert(DespawnedEntity);
            } else {
                Self::despawned_entity(action, true);
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
            Self::despawned_entity(action, true)
        }
        if undo_spawns == matches!(action, CommandAction::Undo | CommandAction::RedoFinalize) {
            if entity.contains::<T>() {
                Self::t(action, true);
            }
            if !action.finalize() {
                if let Some(value) = entity.remove::<Despawned<T>>() {
                    entity.insert(value.0);
                } else {
                    Self::despawned_t(action, false);
                }
            }
        } else {
            if entity.contains::<Despawned<T>>() {
                Self::despawned_t(action, true);
            }
            if !action.finalize() {
                if let Some(value) = entity.remove::<T>() {
                    entity.insert(Despawned(value));
                } else {
                    Self::t(action, false);
                }
            }
        }
    }
    fn resource<T: Resource>(world: &mut World, action: CommandAction, undo_spawns: bool) {
        if undo_spawns == matches!(action, CommandAction::Undo | CommandAction::RedoFinalize) {
            if world.contains_resource::<T>() {
                Self::t(action, true);
            }
            if !action.finalize() {
                if let Some(value) = world.remove_resource::<Despawned<T>>() {
                    world.insert_resource(value.0);
                } else {
                    Self::despawned_t(action, false);
                }
            }
        } else {
            if world.contains_resource::<Despawned<T>>() {
                Self::despawned_t(action, true);
            }
            if !action.finalize() {
                if let Some(value) = world.remove_resource::<T>() {
                    world.insert_resource(Despawned(value));
                } else {
                    Self::t(action, false);
                }
            }
        }
    }
    fn despawned_entity(action: CommandAction, found: bool) {
        if found {
            Self::panic::<()>(action, "`DespawnedEntity` found")
        } else {
            Self::panic::<()>(action, "`DespawnedEntity` not found")
        }
    }
    fn despawned_t(action: CommandAction, found: bool) {
        if found {
            Self::panic(action, "`Despawned<T>` found")
        } else {
            Self::panic(action, "`Despawned<T>` not found")
        }
    }
    fn t(action: CommandAction, found: bool) {
        if found {
            Self::panic(action, "`T` found")
        } else {
            Self::panic(action, "`T` not found")
        }
    }
    fn panic<R>(action: CommandAction, s: &'static str) -> R {
        panic!(
            "Reversible command `{}` failed at \"{action:?}\": \"{s}\"",
            type_name::<Self>()
        );
    }
}

impl<T> CommandPanic for T {}

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

use bevy::{
    ecs::{world::{Mut, EntityMut}, system::Resource},
    log::{error, info, warn},
    prelude::{Commands, ParallelCommands, World, Component},
};
use std::{any::type_name, fmt::{Debug, self}, marker::PhantomData, mem::ManuallyDrop};

use crate::{Despawned, DespawnedEntity};

use super::controller::Controller;

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

/// Trait for reversible commands that are not yet initialized.
pub trait ReversibleCommand: Send + Sync + 'static {
    //returns `Some` if init was successful.
    fn init(self: Box<Self>, world: &mut World) -> Option<Box<dyn ReversibleCommandInitialized>>;
}

impl Debug for dyn ReversibleCommand{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.fmt(f)   
    }
}

/// Trait for reversible commands that are initialized.
pub trait ReversibleCommandInitialized: Send + Sync + 'static {
    /// Redo or undo the command. `Self` needs to track itself if `redo` or `undo`is to be applied. The first call to this function is `undo`.
    fn undo_redo(&mut self, world: &mut World);
    /// Clean up data. `Self` needs to track itself if `redo` or `undo` was called last to keep the vtable size smaller.
    fn finalize(self: Box<Self>, world: &mut World);
    #[cfg(debug)]
    /// Get debug information
    fn debug(&self) -> String{

    }
}

trait CommandPanic{
    fn panic(&self, s: &'static str){
        panic!("Reversible command `{s}` panicked: \"{}\"", type_name::<Self>())
    }
}

impl<T> CommandPanic for T{}

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

use std::{fmt::Debug, any::type_name};
use bevy::{prelude::{Commands, World}, log::{error, warn, info}};
use super::controller::Controller;

mod spawn_component;
mod despawn_component;
mod spawn_resource;
mod despawn_resource;
mod spawn_entity;
mod despawn_entity;

pub use spawn_component::*;
pub use despawn_component::*;
pub use spawn_resource::*;
pub use despawn_resource::*;
pub use spawn_entity::*;
pub use despawn_entity::*;

/// `Commands` wrapper to work with reversible commands.
pub struct ReversibleCommands<'w, 's>(pub (super) Commands<'w, 's>);

impl<'w, 's> ReversibleCommands<'w, 's>{
    /// Add a reversible command
    pub fn add<T: ReversibleCommand>(&mut self, command: T){
        self.0.add(|world: &mut World|{
            let command = command.init(world);
            world
                .resource_mut::<Controller>()
                .next_entry
                .push(Box::new(command));
        })
    }
}

/// Trait for reversible commands that are not yet initialized.
pub trait ReversibleCommand: Send + Sync + 'static{
    /// Type after initialization, typically unequal to `Self` if fields in Self are no longer needed or moved or additional fields are set in the `init` method
    type Initialized: ReversibleCommandInitialized;
    /// Initialize by mutating the world, has to include actions that occur in the `Self::WithoutInitData::redo` method
    fn init(self, world: &mut World) -> Self::Initialized;
}

/// Trait for reversible commands that are initialized.
pub trait ReversibleCommandInitialized: Send + Sync + 'static{
    /// Deploy commands to redo the related actions
    fn redo(&mut self, commands: &mut Commands);
    /// Deploy commands to undo the related actions
    fn undo(&mut self, commands: &mut Commands);
    /// Cleanup commands like despawning buffers before Self is dropped
    fn cleanup(&mut self, commands: &mut Commands);
}

/// Options for errorhandling of the error type `E` in reversible commands.
/// 
/// - `LogError` variant uses bevy's `log::error` macro
/// - `LogError` variant uses bevy's `log::warn` macro
/// - `LogError` variant uses bevy's `log::info` macro
/// 
/// The above variants call their respective macro with these parameters:
/// 
/// `"LogCommand failed: {error:?}, relevant type: {}", type_name::<T>()`
/// 
/// ...where `error` is `&E` and `T` is an additional type that is relevant for this command, like the type of a resource that is tried to be spawned.
/// 
/// A custom error handling can be set with the variant `Custom` which calls a `fn(&E)` in error cases.
pub enum ReversibleCommandErrorHandling<E: Debug>{
    LogError,
    LogWarning,
    LogInfo,
    Custom(fn(&E))
}

impl<E: Debug> ReversibleCommandErrorHandling<E>{
    fn error<T>(&self, error: &E){
        todo!("add another generic which identifies the relevant system, maybe also wrapped in an enum (resource, component, stateless, etc) or enum as a method field");
        match self{
            Self::LogError => error!("LogCommand failed: {error:?}, relevant type: {}", type_name::<T>()),
            Self::LogWarning => warn!("LogCommand failed: {error:?}, relevant type: {}", type_name::<T>()),
            Self::LogInfo => info!("LogCommand failed: {error:?}, relevant type: {}", type_name::<T>()),
            Self::Custom(f) => f(error)
        }
    }
}

impl<E: Debug> Default for ReversibleCommandErrorHandling<E>{
    fn default() -> Self {
        Self::LogError
    }
}
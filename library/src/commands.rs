use std::{fmt::Debug, any::type_name, marker::PhantomData};
use bevy::{prelude::{Commands, World, ParallelCommands}, log::{error, warn, info}};
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

pub(super) type NextCommands<Marker> = Option<Box<dyn FnOnce(ReversibleCommands<Marker>)>>;

/// `Commands` wrapper to work with reversible commands.
pub struct ReversibleCommands<'a, 'w, 's, Marker>{
    commands: &'a ParallelCommands<'w, 's>,
    marker: PhantomData<Marker>
}

impl<'a, 'w, 's, Marker> ReversibleCommands<'a, 'w, 's, Marker>{
    pub(super) fn new(commands: &'a ParallelCommands<'w, 's>) -> Self{
        Self { commands, marker: PhantomData }
    }
    /// Add a reversible command
    pub fn add<T: ReversibleCommand>(&self, command: T){
        self.commands.command_scope(|mut commands|{
            commands.add(|world: &mut World|{
                let command = command.init::<Marker>(world);
                world
                    .resource_mut::<Controller>()
                    .next_entry
                    .push(Box::new(command));
            });
        });
    }
}

/// Trait for reversible commands that are not yet initialized.
pub trait ReversibleCommand: Send + Sync + 'static{
    /// Type after initialization, typically unequal to `Self` if fields in Self are no longer needed or moved or additional fields are set in the `init` method.
    type Initialized: ReversibleCommandInitialized;
    /// Initialize by mutating the world, has to include actions that occur in the `Self::WithoutInitData::redo` method.
    /// 
    /// Generic parameter `M` is the type of the calling reversible system
    fn init<M>(self, world: &mut World) -> Self::Initialized;
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
/// `"LogCommand failed: {error:?} for type {}, issued by reversible system {}", std::any::type_name::<T>(), std::any::type_name::<M>()`
/// 
/// ...where `error` is `&E` and `T` is an additional type that is relevant for this command, like the type of a resource that is tried to be spawned.
/// 
/// A custom error handling can be set with the variant `Custom` which calls a `fn(&E)` in error cases.
pub enum ReversibleCommandErrorHandling<E: Debug>{
    LogError,
    LogWarning,
    LogInfo,
    Custom(Box<dyn Fn(&E) + Send + Sync>)
}

impl<E: Debug> ReversibleCommandErrorHandling<E>{
    fn error<T, M>(&self, error: &E){
        //todo!("add another generic which identifies the relevant system, maybe also wrapped in an enum (resource, component, stateless, etc) or enum as a method field");
        match self{
            Self::LogError => error!("LogCommand failed: {error:?} for type {}, issued by reversible system {}", type_name::<T>(), type_name::<M>()),
            Self::LogWarning => warn!("LogCommand failed: {error:?} for type {}, issued by reversible system {}", type_name::<T>(), type_name::<M>()),
            Self::LogInfo => info!("LogCommand failed: {error:?} for type {}, issued by reversible system {}", type_name::<T>(), type_name::<M>()),
            Self::Custom(f) => f(error)
        }
    }
}

impl<E: Debug> Default for ReversibleCommandErrorHandling<E>{
    fn default() -> Self {
        Self::LogError
    }
}
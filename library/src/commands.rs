use bevy::{
    ecs::world::Mut,
    log::{error, info, warn},
    prelude::{Commands, ParallelCommands, World},
};
use std::{any::type_name, fmt::Debug, marker::PhantomData, mem::ManuallyDrop};

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

pub(super) type NextCommands =
    Box<dyn for<'a> FnOnce(ReversibleWorld<'a>) + Send + Sync>;

/// `Commands` wrapper to work with reversible commands.
pub struct ReversibleWorld<'a> (pub(super) &'a mut World);

impl<'a> ReversibleWorld<'a> {
    /// Add a reversible command
    pub fn add<T: ReversibleCommand>(&mut self, command: T) {
        self.0
            .resource_scope(|world, mut controller: Mut<'_, Controller>| {
                let command = command.init(world);
                controller.push_command(command);
            });
    }
}

/// Trait for reversible commands that are not yet initialized.
pub trait ReversibleCommand: Send + Sync + 'static {
    fn init(self, world: &mut World) -> Box<dyn ReversibleCommandInitialized>;
}

/// Trait for reversible commands that are initialized.
pub trait ReversibleCommandInitialized: Send + Sync + 'static {
    /// Redo the command
    fn redo(&mut self, world: &mut World);
    /// Undo the command
    fn undo(&mut self, world: &mut World);
    /// Remove data that is needed to undo the command
    fn redo_finalize(&mut self, world: &mut World);
    /// Remove data that is needed to redo the command
    fn undo_finalize(&mut self, world: &mut World);
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
            Self::LogError => error!(
                "LogCommand failed: {error:?} for type {}",
                type_name::<T>()
            ),
            Self::LogWarning => warn!(
                "LogCommand failed: {error:?} for type {}",
                type_name::<T>()
            ),
            Self::LogInfo => info!(
                "LogCommand failed: {error:?} for type {}",
                type_name::<T>()
            ),
            Self::Custom(f) => f(error),
        }
    }
}

impl<E: Debug> Default for ReversibleCommandErrorHandling<E> {
    fn default() -> Self {
        Self::LogError
    }
}

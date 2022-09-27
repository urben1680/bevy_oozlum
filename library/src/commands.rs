use std::{fmt::Debug, any::type_name, marker::PhantomData, num::Wrapping, mem::ManuallyDrop};
use bevy::{prelude::{Commands, World, ParallelCommands}, log::{error, warn, info}, ecs::system::Command};
use crate::Ticks;

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

pub(super) type NextCommands<Marker> = Box<dyn FnOnce(ReversibleCommands<Marker>) + Send + Sync + 'static>;

/// `Commands` wrapper to work with reversible commands.
pub struct ReversibleCommands<'a, 'w, 's, Marker: Send + Sync + 'static>{
    commands: &'a mut Commands<'w, 's>, //todo change back to commands and build from parallel commands where needed
    marker: PhantomData<Marker>
}

impl<'a, 'w, 's, Marker: Send + Sync + 'static> ReversibleCommands<'a, 'w, 's, Marker>{
    pub(super) fn new(commands: &'a mut Commands<'w, 's>) -> Self{
        Self { commands, marker: PhantomData }
    }
    pub(super) fn delayed(mut commands: Commands<'w, 's>, command: NextCommands<Marker>, target: Wrapping<Ticks>){
        let delayed = DelayedCommandWrapper::new(command);
        commands.add(move |world: &mut World|{
            let controller = &mut world
                .resource_mut::<Controller>();
            let index = (target - controller.time_stamp()).0 as usize;
            controller.delayed_commands.get_mut(index).unwrap().push(Box::new(delayed));               
        })
    }
    /// Add a reversible command
    pub fn add<T: ReversibleCommand>(&mut self, command: T){
        self.commands.add(|world: &mut World|{
            let command = command.init::<Marker>(world);
            world
                .resource_mut::<Controller>()
                .next_entry
                .push(Box::new(command));
        });
    }
}

pub trait CommandsScope<'w, 's>{
    fn get_command_scope<R>(self, f: impl FnOnce(Commands) -> R) -> R;
}

impl<'w, 's> CommandsScope<'w, 's> for Commands<'w, 's>{
    fn get_command_scope<R>(self, f: impl FnOnce(Commands) -> R) -> R{
        f(self)
    }
}

impl<'w, 's> CommandsScope<'w, 's> for &ParallelCommands<'w, 's>{
    fn get_command_scope<R>(self, f: impl FnOnce(Commands) -> R) -> R{
        self.command_scope(f)
    }
}

pub(super) struct DelayedCommandWrapper<Marker: Send + Sync + 'static>{
    command: ManuallyDrop<NextCommands<Marker>>
}

impl<Marker: Send + Sync + 'static> DelayedCommandWrapper<Marker>{
    pub(super) fn new(command: NextCommands<Marker>) -> Self{
        Self { command: ManuallyDrop::new(command) }
    }
}

pub(super) trait DelayedCommand: Send + Sync + 'static{
    /// SAFETY: call only once, see https://doc.rust-lang.org/std/mem/struct.ManuallyDrop.html#method.take
    unsafe fn init(&mut self, commands: &mut Commands);
}

impl<Marker: Send + Sync +'static> DelayedCommand for DelayedCommandWrapper<Marker>{
    unsafe fn init(&mut self, commands: &mut Commands) {
        ManuallyDrop::take(&mut self.command)(ReversibleCommands::<Marker>::new(commands));
    }
}

trait DelayedCommands: ReversibleCommand{
    fn init(self, world: &mut World) -> Self::Initialized;
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
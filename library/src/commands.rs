use std::collections::VecDeque;

use bevy::{
    ecs::{
        event::Event,
        observer::{TriggerEvent, TriggerTargets},
        system::{Commands, EntityCommands, Resource},
        world::{DeferredWorld, FromWorld, World},
    },
    utils::synccell::SyncCell,
};

use crate::{
    log::{OutOfLog, TransitionsLog, WithLoggedAt},
    meta::{Direction, RevMeta},
};

mod bundle;
pub mod hook;
pub mod observer;

// todo: spawn/despawn with entity disabling https://github.com/bevyengine/bevy/issues/11090
// todo: commands implementors https://docs.rs/bevy/latest/bevy/ecs/world/trait.Command.html#implementors
// todo: untyped take component https://github.com/bevyengine/bevy/issues/15350

pub trait RevCommands {
    fn rev_add<Marker>(&mut self, command: impl RevCommand<Marker>);
    fn rev_init_resource<R: Resource + FromWorld>(&mut self);
    fn rev_insert_resource<R: Resource>(&mut self, resource: R);
    fn rev_remove_resource<R: Resource>(&mut self);
    fn rev_trigger(&mut self, event: impl Event + Clone);
    fn rev_trigger_targets(&mut self, event: impl Event + Clone, targets: impl TriggerTargets);
}

fn buffer_rev_command(world: &mut DeferredWorld, command: impl InitializedRevCommand) {
    let command: Box<dyn InitializedRevCommand> = Box::new(command);
    let command = SyncCell::new(command);
    world
        .get_resource_mut::<RevCommandBuffer>()
        .expect("todo")
        .0
        .push_back(command);
}

impl RevCommands for Commands<'_, '_> {
    fn rev_add<Marker>(&mut self, command: impl RevCommand<Marker>) {
        self.add(|world: &mut World| {
            if let Some(command) = command.rev_apply(world) {
                buffer_rev_command(&mut world.into(), command)
            }
        })
    }
    fn rev_init_resource<R: Resource + FromWorld>(&mut self) {
        self.rev_add(|world: &mut World| {
            let initiialized = ResourceSwap(world.remove_resource::<R>());
            world.init_resource::<R>();
            initiialized
        })
    }
    fn rev_insert_resource<R: Resource>(&mut self, resource: R) {
        self.rev_add(|world: &mut World| {
            let initiialized = ResourceSwap(world.remove_resource::<R>());
            world.insert_resource(resource);
            initiialized
        })
    }
    fn rev_remove_resource<R: Resource>(&mut self) {
        self.rev_add(|world: &mut World| {
            world
                .remove_resource::<R>()
                .map(|resource| ResourceSwap(Some(resource)))
        })
    }
    fn rev_trigger(&mut self, event: impl Event + Clone) {
        self.rev_add(TriggerEvent { event, targets: () })
    }
    fn rev_trigger_targets(&mut self, event: impl Event + Clone, targets: impl TriggerTargets) {
        self.rev_add(TriggerEvent { event, targets })
    }
}

struct ResourceSwap<R: Resource>(Option<R>);

impl<R: Resource> InitializedRevCommand for ResourceSwap<R> {
    fn undo(&mut self, world: &mut World) {
        match (self.0.as_mut(), world.get_resource_mut::<R>()) {
            (Some(r1), Some(mut r2)) => core::mem::swap(r1, &mut *r2),
            (Some(_), None) => world.insert_resource(self.0.take().unwrap()),
            (None, Some(_)) => self.0 = world.remove_resource::<R>(),
            (None, None) => {}
        }
    }
    fn redo(&mut self, world: &mut World) {
        self.undo(world)
    }
}

pub trait RevEntityCommands {}

impl<'a> RevEntityCommands for EntityCommands<'a> {}

pub trait RevCommand<Marker>: Send + 'static {
    fn rev_apply(self, world: &mut World) -> Option<impl InitializedRevCommand>;
}

impl<T: InitializedRevCommand, F: FnOnce(&mut World) -> Option<T> + Send + 'static>
    RevCommand<fn(&mut World) -> Option<T>> for F
{
    fn rev_apply(self, world: &mut World) -> Option<impl InitializedRevCommand> {
        self(world)
    }
}

impl<T: InitializedRevCommand, F: FnOnce(&mut World) -> T + Send + 'static>
    RevCommand<fn(&mut World) -> T> for F
{
    fn rev_apply(self, world: &mut World) -> Option<impl InitializedRevCommand> {
        Some(self(world))
    }
}

impl<T: InitializedRevCommand> RevCommand<Option<T>> for Option<T> {
    fn rev_apply(self, _world: &mut World) -> Option<impl InitializedRevCommand> {
        self
    }
}

impl<T: InitializedRevCommand> RevCommand<T> for T {
    fn rev_apply(self, _world: &mut World) -> Option<impl InitializedRevCommand> {
        Some(self)
    }
}

pub trait InitializedRevCommand: Send + 'static {
    fn undo(&mut self, world: &mut World);
    fn undone_finalize(self: Box<Self>, _world: &mut World) {}
    fn redo(&mut self, world: &mut World);
    fn redone_finalize(self: Box<Self>, _world: &mut World) {}
}

#[derive(Resource, Default)]
pub(crate) struct RevCommandBuffer(VecDeque<SyncCell<Box<dyn InitializedRevCommand>>>);

#[derive(Default)]
pub struct CommandsLog(TransitionsLog<SyncCell<Box<dyn InitializedRevCommand>>, WithLoggedAt>);

#[derive(Clone, Debug)]
pub enum CommandsLogErr {
    RevMetaMissing,
    RevMetaWrongDirection(RevMeta),
    OutOfLog(RevMeta),
}

impl CommandsLog {
    pub fn forward(&mut self, world: &mut World) -> Result<(), CommandsLogErr> {
        let meta = world
            .get_resource::<RevMeta>()
            .ok_or(CommandsLogErr::RevMetaMissing)?
            .clone();
        match meta.get_direction() {
            Some(Direction::Forward { log: false }) => {
                for command in self.0.drain_future().0.rev() {
                    SyncCell::to_inner(command).undone_finalize(world);
                }
                for command in self.0.drain_past_by_timestamp(meta.log_range().start) {
                    SyncCell::to_inner(command).redone_finalize(world);
                }
                let mut buffer = world.get_resource_or_insert_with(RevCommandBuffer::default);
                if !buffer.0.is_empty() {
                    self.0.push_present(|mut log| {
                        log.append(&mut buffer.0);
                        meta.now()
                    });
                }
                Ok(())
            }
            Some(Direction::Forward { log: true }) => {
                for command in self
                    .0
                    .forward_log()
                    .map_err(|OutOfLog| CommandsLogErr::OutOfLog(meta))?
                    .into_iter()
                {
                    command.get().redo(world);
                }
                Ok(())
            }
            _ => Err(CommandsLogErr::RevMetaWrongDirection(meta)),
        }
    }
    pub fn backward(&mut self, world: &mut World) -> Result<(), CommandsLogErr> {
        let meta = world
            .get_resource::<RevMeta>()
            .ok_or(CommandsLogErr::RevMetaMissing)?;
        if meta.get_direction() != Some(Direction::BackwardLog) {
            return Err(CommandsLogErr::RevMetaWrongDirection(meta.clone()));
        }
        for command in self
            .0
            .backward_log()
            .map_err(|OutOfLog| CommandsLogErr::OutOfLog(meta.clone()))?
            .into_iter()
        {
            command.get().undo(world);
        }
        Ok(())
    }
}

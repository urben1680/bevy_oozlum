use std::collections::VecDeque;

use bevy::{
    prelude::{Commands, Resource, World},
    utils::synccell::SyncCell,
};

use crate::{
    log::{TransitionsLog, WithTimestamp},
    meta::{Direction, RevMeta},
};

pub trait RevCommands {
    fn add_rev<Marker>(&mut self, command: impl RevCommand<Marker>);
    // todo: entity commands
}

impl RevCommands for Commands<'_, '_> {
    fn add_rev<Marker>(&mut self, command: impl RevCommand<Marker>) {
        self.add(|world: &mut World| {
            if let Some(command) = command.apply(world) {
                let command: Box<dyn InitializedRevCommand> = Box::new(command);
                let command = SyncCell::new(command);
                world
                    .get_resource_or_insert_with(RevCommandBuffer::default)
                    .0
                    .push_back(command);
            }
        })
    }
}

pub trait RevCommand<Marker>: Send + 'static {
    fn apply(self, world: &mut World) -> Option<impl InitializedRevCommand>;
}

impl<T: InitializedRevCommand, F: FnOnce(&mut World) -> Option<T> + Send + 'static> RevCommand<fn(&mut World) -> Option<T>> for F {
    fn apply(self, world: &mut World) -> Option<impl InitializedRevCommand> {
        self(world)
    }
}

impl<T: InitializedRevCommand, F: FnOnce(&mut World) -> T + Send + 'static> RevCommand<fn(&mut World) -> T> for F {
    fn apply(self, world: &mut World) -> Option<impl InitializedRevCommand> {
        Some(self(world))
    }
}

impl<T: InitializedRevCommand> RevCommand<Option<T>> for Option<T> {
    fn apply(self, _world: &mut World) -> Option<impl InitializedRevCommand> {
        self
    }
}

impl<T: InitializedRevCommand> RevCommand<T> for T {
    fn apply(self, _world: &mut World) -> Option<impl InitializedRevCommand> {
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
struct RevCommandBuffer(VecDeque<SyncCell<Box<dyn InitializedRevCommand>>>);

#[derive(Default)]
pub(crate) struct CommandsLog(
    TransitionsLog<SyncCell<Box<dyn InitializedRevCommand>>, WithTimestamp>,
);

impl CommandsLog {
    pub(crate) fn forward(&mut self, world: &mut World) {
        let meta = world
            .get_resource::<RevMeta>()
            .expect(RevMeta::EXIST_MSG)
            .clone();
        if meta.direction() == Direction::Forward {
            for command in self.0.drain_future_transitions().rev() {
                SyncCell::to_inner(command).undone_finalize(world);
            }
            for command in self
                .0
                .pop_front_by_timestamp(&meta)
                .into_iter()
                .flat_map(|entry| entry.data)
            {
                SyncCell::to_inner(command).redone_finalize(world);
            }
            let mut buffer = world.get_resource_or_insert_with(RevCommandBuffer::default);
            if !buffer.0.is_empty() {
                let _infallibe = self.0.push_back(|mut log| {
                    log.append(&mut buffer.0);
                    meta.now().into()
                });
            }
        } else {
            // == Direction::ForwardLog
            for command in self
                .0
                .forward_log()
                .expect("todo not out of log")
                .into_iter()
            {
                command.get().redo(world);
            }
        }
    }
    pub(crate) fn backward(&mut self, world: &mut World) {
        for command in self
            .0
            .backward_log()
            .expect("todo not out of log")
            .into_iter()
            .rev()
        {
            command.get().undo(world);
        }
    }
}

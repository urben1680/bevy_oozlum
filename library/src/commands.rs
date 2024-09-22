use std::{collections::VecDeque, ops::Deref};

use bevy::{
    ecs::{observer::{TriggerEvent, TriggerTargets}, system::EntityCommands}, prelude::{Commands, Event, FromWorld, Resource, World}, utils::synccell::SyncCell
};

use crate::{
    log::{OutOfLog, StateLog, TransitionsLog, WithTimestamp},
    meta::{Direction, RevMeta},
};

mod bundle;
mod observer;

// todo: spawn/despawn with entity disabling https://github.com/bevyengine/bevy/issues/11090
// todo: commands implementors https://docs.rs/bevy/latest/bevy/ecs/world/trait.Command.html#implementors
// todo: untyped take component https://github.com/bevyengine/bevy/issues/15350

pub trait RevCommands {
    fn rev_add<Marker>(&mut self, command: impl RevCommand<Marker>);
    fn rev_init_resource<R: Resource + FromWorld>(&mut self);
    fn rev_insert_resource<R: Resource>(&mut self, resource: R);
    fn rev_remove_resource<R: Resource>(&mut self);
    fn rev_trigger(&mut self, event: impl Event);
    fn rev_trigger_targets(&mut self, event: impl Event, targets: impl TriggerTargets);
}

impl RevCommands for Commands<'_, '_> {
    fn rev_add<Marker>(&mut self, command: impl RevCommand<Marker>) {
        self.add(|world: &mut World| {
            if let Some(command) = command.rev_apply(world) {
                let command: Box<dyn InitializedRevCommand> = Box::new(command);
                let command = SyncCell::new(command);
                world
                    .get_resource_or_insert_with(RevCommandBuffer::default)
                    .0
                    .push_back(command);
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
            world.remove_resource::<R>().map(|resource| ResourceSwap(Some(resource)))
        })
    }
    fn rev_trigger(&mut self, event: impl Event) {
        
    }
    fn rev_trigger_targets(&mut self, event: impl Event, targets: impl TriggerTargets) {
        
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

/*
Event does not impl Clone
Could add observer for event so it is passed on to the log
- pro: ~~no Clone bound on event needed~~ wrong, event is only available by reference

- event can be mutated by the observer, needs to be reversible
-- impl RevCommand only for RevEventMut<E>(StateLog<E>) and RevEvent<E>(E)
--- downside: makes api uglier if bound is not Event but RevEvent: Event to only support immutable and logged event
--- try to support more logs

-- roadblock for mutable rev observer: ordering api is missing https://github.com/bevyengine/bevy/issues/14890
--- do not offer logged events yet, warn about mutating events
--- or offer an event wrapper that offers a data log where each observer can set data and only read data of previous observers
---- data size: TriggerTargets.len()?
- TriggerTargets could be reversed by rev command! but no order can be specified with rev_trigger (without targets)
-- this is just an implemention detail
*/

/// Wraps an event to only allow immutable access. 
pub struct RevEvent<E>(E);

impl<E> Deref for RevEvent<E> {
    type Target = E;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

pub struct RevEventMut<E: Event>(pub StateLog<E>);

impl<E: Event, Targets: TriggerTargets> RevCommand<()> for TriggerEvent<RevEvent<E>, Targets> {
    fn rev_apply(self, world: &mut World) -> Option<impl InitializedRevCommand> {
        todo!()
    }
}

impl<E: Event, Targets: TriggerTargets> RevCommand<()> for TriggerEvent<RevEventMut<E>, Targets> {
    fn rev_apply(self, world: &mut World) -> Option<impl InitializedRevCommand> {
        todo!()
    }
}

pub trait RevEntityCommands {

}

impl<'a> RevEntityCommands for EntityCommands<'a> {

}

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
struct RevCommandBuffer(VecDeque<SyncCell<Box<dyn InitializedRevCommand>>>);

#[derive(Default)]
pub struct CommandsLog(TransitionsLog<SyncCell<Box<dyn InitializedRevCommand>>, WithTimestamp>);

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
            Some(Direction::Forward) => {
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
            Some(Direction::ForwardLog) => {
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

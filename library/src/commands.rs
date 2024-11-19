use std::collections::VecDeque;

use bevy::{
    ecs::{
        bundle::Bundle,
        event::Event,
        observer::{TriggerEvent, TriggerTargets},
        system::{Commands, EntityCommands, IntoObserverSystem, Resource},
        world::{DeferredWorld, FromWorld, World},
    },
    utils::{default, synccell::SyncCell},
};

use crate::{
    log::{OutOfLog, TransitionsLog},
    meta::{RevDirection, RevMeta},
    observer::{ObserverLog, RevEvent},
    RevFrame,
};

// todo: spawn/despawn with entity disabling https://github.com/bevyengine/bevy/issues/11090
// todo: commands implementors https://docs.rs/bevy/latest/bevy/ecs/world/trait.Command.html#implementors
// todo: untyped take component https://github.com/bevyengine/bevy/issues/15350

pub trait RevCommands {
    fn rev_add_observer<E, B, M>(
        &mut self,
        system: impl IntoObserverSystem<RevEvent<E>, B, M>,
    ) -> EntityCommands<'_>
    where
        E: Event + Clone,
        B: Bundle;
    fn rev_queue<Marker>(&mut self, command: impl RevCommand<Marker>);
    fn rev_init_resource<R: Resource + FromWorld>(&mut self);
    fn rev_insert_resource<R: Resource>(&mut self, resource: R);
    fn rev_remove_resource<R: Resource>(&mut self);
    fn rev_trigger(&mut self, event: impl Event + Clone);
    fn rev_trigger_targets(
        &mut self,
        event: impl Event + Clone,
        targets: impl TriggerTargets + Send + 'static,
    );
}

pub(crate) fn buffer_rev_command<T: RevCommandLog>(world: &mut DeferredWorld, command: T) {
    let command: Box<dyn RevCommandLog> = Box::new(command);
    let buffer = &mut world
        .get_resource_mut::<RevCommandBuffer>()
        .expect("todo")
        .0;
    SyncCell::get(buffer).push_back(command);
}

impl RevCommands for Commands<'_, '_> {
    fn rev_add_observer<E, B, M>(
        &mut self,
        system: impl IntoObserverSystem<RevEvent<E>, B, M>,
    ) -> EntityCommands<'_>
    where
        E: Event + Clone,
        B: Bundle,
    {
        self.init_resource::<ObserverLog<E>>();
        self.add_observer(system)
    }
    fn rev_queue<Marker>(&mut self, command: impl RevCommand<Marker>) {
        self.queue(|world: &mut World| {
            if let Some(command) = command.rev_apply(world) {
                buffer_rev_command(&mut world.into(), command)
            }
        })
    }
    fn rev_init_resource<R: Resource + FromWorld>(&mut self) {
        self.rev_queue(|world: &mut World| {
            (!world.contains_resource::<R>()).then(|| {
                world.init_resource::<R>();
                ResourceSwap::<R>(None)
            })
        })
    }
    fn rev_insert_resource<R: Resource>(&mut self, resource: R) {
        self.rev_queue(|world: &mut World| {
            let initiialized = ResourceSwap(world.remove_resource::<R>());
            world.insert_resource(resource);
            initiialized
        })
    }
    fn rev_remove_resource<R: Resource>(&mut self) {
        self.rev_queue(|world: &mut World| {
            world
                .remove_resource::<R>()
                .map(|resource| ResourceSwap(Some(resource)))
        })
    }
    fn rev_trigger(&mut self, event: impl Event + Clone) {
        self.rev_queue(TriggerEvent { event, targets: () })
    }
    fn rev_trigger_targets(
        &mut self,
        event: impl Event + Clone,
        targets: impl TriggerTargets + Send + 'static,
    ) {
        self.rev_queue(TriggerEvent { event, targets })
    }
}

pub trait RevEntityCommands {
    fn rev_observe<E, B, M>(
        &mut self,
        system: impl IntoObserverSystem<RevEvent<E>, B, M>,
    ) -> &mut Self
    where
        E: Event + Clone,
        B: Bundle;
}

impl RevEntityCommands for EntityCommands<'_> {
    fn rev_observe<E, B, M>(
        &mut self,
        system: impl IntoObserverSystem<RevEvent<E>, B, M>,
    ) -> &mut Self
    where
        E: Event + Clone,
        B: Bundle,
    {
        self.commands().init_resource::<ObserverLog<E>>();
        self.observe(system)
    }
}

struct ResourceSwap<R: Resource>(Option<R>);

impl<R: Resource> RevCommandLog for ResourceSwap<R> {
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

pub trait RevCommand<Marker>: Send + 'static {
    fn rev_apply(self, world: &mut World) -> Option<impl RevCommandLog>;
}

impl<T: RevCommandLog, F: FnOnce(&mut World) -> Option<T> + Send + 'static>
    RevCommand<fn(&mut World) -> Option<T>> for F
{
    fn rev_apply(self, world: &mut World) -> Option<impl RevCommandLog> {
        self(world)
    }
}

impl<T: RevCommandLog, F: FnOnce(&mut World) -> T + Send + 'static> RevCommand<fn(&mut World) -> T>
    for F
{
    fn rev_apply(self, world: &mut World) -> Option<impl RevCommandLog> {
        Some(self(world))
    }
}

impl<T: RevCommandLog> RevCommand<Option<T>> for Option<T> {
    fn rev_apply(self, _world: &mut World) -> Option<impl RevCommandLog> {
        self
    }
}

impl<T: RevCommandLog> RevCommand<T> for T {
    fn rev_apply(self, _world: &mut World) -> Option<impl RevCommandLog> {
        Some(self)
    }
}

pub trait RevCommandLog: Send + 'static {
    fn undo(&mut self, world: &mut World);
    fn undone_finalize(self: Box<Self>, _world: &mut World) {}
    fn redo(&mut self, world: &mut World);
    fn redone_finalize(self: Box<Self>, _world: &mut World) {}
}

impl<F: FnMut(&mut World, bool) + Send + 'static> RevCommandLog for F {
    fn undo(&mut self, world: &mut World) {
        self(world, false)
    }
    fn redo(&mut self, world: &mut World) {
        self(world, true)
    }
}

#[derive(Resource)]
struct RevCommandBuffer(SyncCell<VecDeque<Box<dyn RevCommandLog>>>);

impl Default for RevCommandBuffer {
    fn default() -> Self {
        Self(SyncCell::new(default()))
    }
}

pub(crate) fn init_commands_buffer(world: &mut World) {
    world.init_resource::<RevCommandBuffer>();
}

pub struct CommandsLog(SyncCell<TransitionsLog<Box<dyn RevCommandLog>, RevFrame>>);

impl Default for CommandsLog {
    fn default() -> Self {
        Self(SyncCell::new(default()))
    }
}

#[derive(Clone, Debug)]
pub enum CommandsLogErr {
    RevMetaMissing,
    RevMetaWrongDirection(RevMeta),
    RevCommandBufferMissing(RevMeta),
    OutOfLog(RevMeta),
}

impl CommandsLog {
    pub fn forward(&mut self, world: &mut World) -> Result<(), CommandsLogErr> {
        let meta = world
            .get_resource::<RevMeta>()
            .ok_or(CommandsLogErr::RevMetaMissing)?
            .clone();
        let log = SyncCell::get(&mut self.0);
        match meta.get_direction() {
            Some(RevDirection::NotLog) => {
                for command in log.drain_future().0.rev() {
                    command.undone_finalize(world);
                }
                for command in log.truncate_future_drain_past_by_logged_at(&meta) {
                    command.redone_finalize(world);
                }
                let mut buffer = world
                    .get_resource_mut::<RevCommandBuffer>()
                    .ok_or_else(|| CommandsLogErr::RevCommandBufferMissing(meta.clone()))?;
                let buffer = SyncCell::get(&mut buffer.0);
                if !buffer.is_empty() {
                    log.push_present(|mut log| {
                        log.append(buffer);
                        meta.present_world_state()
                    });
                }
                Ok(())
            }
            Some(RevDirection::ForwardLog) => {
                for command in log
                    .forward_log()
                    .map_err(|OutOfLog| CommandsLogErr::OutOfLog(meta))?
                    .into_iter()
                {
                    command.redo(world);
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
        if meta.get_direction() != Some(RevDirection::BackwardLog) {
            return Err(CommandsLogErr::RevMetaWrongDirection(meta.clone()));
        }
        let log = SyncCell::get(&mut self.0);
        for command in log
            .backward_log()
            .map_err(|OutOfLog| CommandsLogErr::OutOfLog(meta.clone()))?
            .into_iter()
            .rev()
        {
            command.undo(world);
        }
        Ok(())
    }
    pub fn reduce_logged_at(&mut self, world: &mut World, meta: &RevMeta) {
        let log = SyncCell::get(&mut self.0);
        for command in log.truncate_future_drain_past_by_logged_at(meta) {
            command.redone_finalize(world);
        }
    }
}

use std::collections::VecDeque;

use bevy::{
    ecs::{
        change_detection::ResMut,
        system::{Commands, Resource, SystemParam},
        world::{DeferredWorld, FromWorld, World},
    },
    utils::{default, synccell::SyncCell},
};

use crate::{
    log::{OutOfLog, TransitionsLog},
    meta::{RevDirection, RevMeta},
    RevFrame,
};

pub trait BuffersUndoRedo {
    fn buffer_undo_redo(&mut self, undo_redo: impl UndoRedo);
}

impl BuffersUndoRedo for World {
    fn buffer_undo_redo(&mut self, undo_redo: impl UndoRedo) {
        DeferredWorld::buffer_undo_redo(&mut self.into(), undo_redo);
    }
}

impl<'w> BuffersUndoRedo for DeferredWorld<'w> {
    fn buffer_undo_redo(&mut self, undo_redo: impl UndoRedo) {
        let undo_redo: Box<dyn UndoRedo> = Box::new(undo_redo);
        let buffer = &mut self
            .get_resource_mut::<UndoRedoBufferSealed>()
            .expect("todo")
            .0;
        SyncCell::get(buffer).push_back(undo_redo);
    }
}

#[derive(SystemParam)]
pub struct UndoRedoBuffer<'w> {
    buffer: ResMut<'w, UndoRedoBufferSealed>,
}

impl<'w> BuffersUndoRedo for UndoRedoBuffer<'w> {
    fn buffer_undo_redo(&mut self, undo_redo: impl UndoRedo) {
        let undo_redo: Box<dyn UndoRedo> = Box::new(undo_redo);
        SyncCell::get(&mut self.buffer.0).push_back(undo_redo);
    }
}

impl UndoRedoBuffer<'static> {
    /// Initializes an internal resource needed for reversible commands to be logged by [`CommandsLog`].
    ///
    /// This usually does not need to be called because that is already done by [`RevSystemsPlugin`](crate::app::RevSystemsPlugin).
    pub fn init(world: &mut World) {
        world.init_resource::<UndoRedoBufferSealed>();
    }
}

pub trait RevCommands {
    fn rev_queue<Marker>(&mut self, command: impl RevCommand<Marker>);
    fn rev_init_resource<R: Resource + FromWorld>(&mut self);
    fn rev_insert_resource<R: Resource>(&mut self, resource: R);
    fn rev_remove_resource<R: Resource>(&mut self);
}

impl RevCommands for Commands<'_, '_> {
    fn rev_queue<Marker>(&mut self, command: impl RevCommand<Marker>) {
        self.queue(|world: &mut World| {
            if let Some(undo_redo) = command.rev_apply(world) {
                world.buffer_undo_redo(undo_redo)
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
}
struct ResourceSwap<R: Resource>(Option<R>);

impl<R: Resource> UndoRedo for ResourceSwap<R> {
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
    fn rev_apply(self, world: &mut World) -> Option<impl UndoRedo>;
}

impl<T: UndoRedo, F: FnOnce(&mut World) -> Option<T> + Send + 'static>
    RevCommand<fn(&mut World) -> Option<T>> for F
{
    fn rev_apply(self, world: &mut World) -> Option<impl UndoRedo> {
        self(world)
    }
}

impl<T: UndoRedo, F: FnOnce(&mut World) -> T + Send + 'static> RevCommand<fn(&mut World) -> T>
    for F
{
    fn rev_apply(self, world: &mut World) -> Option<impl UndoRedo> {
        Some(self(world))
    }
}

impl<T: UndoRedo> RevCommand<Option<T>> for Option<T> {
    fn rev_apply(self, _world: &mut World) -> Option<impl UndoRedo> {
        self
    }
}

impl<T: UndoRedo> RevCommand<T> for T {
    fn rev_apply(self, _world: &mut World) -> Option<impl UndoRedo> {
        Some(self)
    }
}

pub trait UndoRedo: Send + 'static {
    fn undo(&mut self, world: &mut World);
    fn undone_finalize(self: Box<Self>, _world: &mut World) {}
    fn redo(&mut self, world: &mut World);
    fn redone_finalize(self: Box<Self>, _world: &mut World) {}
}

impl<F: FnMut(&mut World, bool) + Send + 'static> UndoRedo for F {
    fn undo(&mut self, world: &mut World) {
        self(world, false)
    }
    fn redo(&mut self, world: &mut World) {
        self(world, true)
    }
}

// uses a VecDeque so the actual log can use `VecDeque::append`
#[derive(Resource)]
struct UndoRedoBufferSealed(SyncCell<VecDeque<Box<dyn UndoRedo>>>);

impl Default for UndoRedoBufferSealed {
    fn default() -> Self {
        Self(SyncCell::new(default()))
    }
}

pub struct CommandsLog(SyncCell<TransitionsLog<Box<dyn UndoRedo>, RevFrame>>);

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
                // should this be reversed too? recent commands may rely on side effects of older commands that are affected here
                for command in log.truncate_future_drain_past_by_logged_at(&meta) {
                    command.redone_finalize(world);
                }
                let mut buffer = world
                    .get_resource_mut::<UndoRedoBufferSealed>()
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

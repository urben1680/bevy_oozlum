use std::{collections::VecDeque, fmt::Debug};

use bevy::{
    ecs::{
        system::Resource,
        world::{DeferredWorld, World},
    },
    utils::{default, synccell::SyncCell},
};

use crate::{
    frame::RevFrame,
    log::{OutOfLog, TransitionsLog},
    meta::{RevDirection, RevMeta},
};

mod commands;
// todo: mod entity_commands

pub use commands::RevCommands;

pub trait BuffersUndoRedo {
    /// Should only be called in sync points of reversible schedules, for example in:
    /// - Commands
    /// - Observers
    /// - Hooks
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
        let buffer = &mut self.get_resource_mut::<UndoRedoBuffer>().expect("todo").0;
        SyncCell::get(buffer).push_back(undo_redo);
    }
}

/// For usages in reversible observer systems.
///
/// Commands and hooks can buffer [`UndoRedo`] implementors via [`&mut World`](World)/[`DeferredWorld`] instead.
///
/// Do not remove or overwrite this resource.
// uses a VecDeque so `CommandsLog` can use `VecDeque::append`
#[derive(Resource)]
pub struct UndoRedoBuffer(SyncCell<VecDeque<Box<dyn UndoRedo>>>);

impl BuffersUndoRedo for UndoRedoBuffer {
    fn buffer_undo_redo(&mut self, undo_redo: impl UndoRedo) {
        let undo_redo: Box<dyn UndoRedo> = Box::new(undo_redo);
        SyncCell::get(&mut self.0).push_back(undo_redo);
    }
}

impl Default for UndoRedoBuffer {
    fn default() -> Self {
        Self(SyncCell::new(default()))
    }
}

impl UndoRedoBuffer {
    pub fn is_empty(&mut self) -> bool {
        self.0.get().is_empty()
    }
}

pub trait UndoRedo: Send + 'static {
    fn undo(&mut self, world: &mut World);
    fn redo(&mut self, world: &mut World);
    fn finalize(self: Box<Self>, _world: &mut World, _undone: bool) {}
}

impl<F: FnMut(&mut World, bool) + Send + 'static> UndoRedo for F {
    fn undo(&mut self, world: &mut World) {
        self(world, false)
    }
    fn redo(&mut self, world: &mut World) {
        self(world, true)
    }
}

pub struct UndoRedoLog(SyncCell<TransitionsLog<Box<dyn UndoRedo>, RevFrame>>);

impl Default for UndoRedoLog {
    fn default() -> Self {
        Self(SyncCell::new(default()))
    }
}

#[derive(Clone, Debug)]
pub enum UndoRedoErr {
    RevMetaMissing,
    RevMetaWrongDirection(RevMeta),
    UndoRedoBufferMissing(RevMeta),
    OutOfLog(RevMeta),
}

impl UndoRedoLog {
    pub fn forward(&mut self, world: &mut World) -> Result<(), UndoRedoErr> {
        let meta = world
            .get_resource::<RevMeta>()
            .ok_or(UndoRedoErr::RevMetaMissing)?
            .clone();
        let log = SyncCell::get(&mut self.0);
        match meta.get_direction() {
            Some(RevDirection::NotLog) => {
                for command in log.drain_future().0.rev() {
                    command.finalize(world, true);
                }
                // should this be reversed too? recent commands may rely on side effects of older commands that are affected here
                for command in log.truncate_future_drain_past_by_logged_at(&meta) {
                    command.finalize(world, false);
                }
                let mut buffer = world
                    .get_resource_mut::<UndoRedoBuffer>()
                    .ok_or_else(|| UndoRedoErr::UndoRedoBufferMissing(meta.clone()))?;
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
                    .map_err(|OutOfLog| UndoRedoErr::OutOfLog(meta))?
                    .into_iter()
                {
                    command.redo(world);
                }
                Ok(())
            }
            _ => Err(UndoRedoErr::RevMetaWrongDirection(meta)),
        }
    }
    pub fn backward(&mut self, world: &mut World) -> Result<(), UndoRedoErr> {
        let meta = world
            .get_resource::<RevMeta>()
            .ok_or(UndoRedoErr::RevMetaMissing)?;
        if meta.get_direction() != Some(RevDirection::BackwardLog) {
            return Err(UndoRedoErr::RevMetaWrongDirection(meta.clone()));
        }
        let log = SyncCell::get(&mut self.0);
        for command in log
            .backward_log()
            .map_err(|OutOfLog| UndoRedoErr::OutOfLog(meta.clone()))?
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
            command.finalize(world, false);
        }
    }
}

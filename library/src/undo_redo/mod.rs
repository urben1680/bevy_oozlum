use std::{collections::VecDeque, fmt::Debug};

use bevy::{
    ecs::{
        system::Commands,
        system::Resource,
        world::{DeferredWorld, World},
    },
    utils::synccell::SyncCell,
};

use crate::{
    log::{DenseTransitionsLog, FrameTransitionLog},
    meta::{RevDirection, RevMeta},
};

mod commands;
// todo: mod entity_commands

pub use commands::RevCommands;

pub trait BuffersUndoRedo {
    /// Buffers an [`UndoRedo`] implementor in a resource to be collected by the reversible system's state during sync points.
    ///
    /// Logic applied in sync points are in:
    /// - commands
    /// - hooks
    /// - observers
    /// - [`SystemParam::apply`](bevy::ecs::system::SystemParam::apply)
    /// - [`SystemBuffer::apply`](bevy::ecs::system::SystemBuffer::apply)
    /// - [`System::apply_deferred`](bevy::ecs::system::System::apply_deferred)
    ///
    /// Note that the sync point **must** belong to a reversible system.
    /// todo: lay out situations where this is not true (trigger in non-reversible systems, queue commands in hooks/observers)
    /// The effect should be immediate in the sync point. Because of this, refer the following table for how to call this method:
    ///
    /// | | Sync Point | Non-Observer System |
    /// | - | - | - |
    /// | [`&mut World`](World) | ✅ | ❌ |
    /// | [`DeferredWorld`] | ✅ | ❌ |
    /// | [`UndoRedoBuffer`] | ✅ | ❌ |
    /// | [`Commands`] | ❌ | ✅ |
    fn buffer_undo_redo(&mut self, undo_redo: impl UndoRedo);
}

impl BuffersUndoRedo for World {
    fn buffer_undo_redo(&mut self, undo_redo: impl UndoRedo) {
        DeferredWorld::buffer_undo_redo(&mut self.into(), undo_redo);
    }
}

impl<'w> BuffersUndoRedo for DeferredWorld<'w> {
    fn buffer_undo_redo(&mut self, undo_redo: impl UndoRedo) {
        self.get_resource_mut::<UndoRedoBuffer>()
            .expect("todo")
            .buffer_undo_redo(undo_redo);
    }
}

impl<'w, 's> BuffersUndoRedo for Commands<'w, 's> {
    fn buffer_undo_redo(&mut self, undo_redo: impl UndoRedo) {
        self.queue(move |world: &mut World| world.buffer_undo_redo(undo_redo))
    }
}
/// For usages in reversible observer systems.
///
/// Commands and hooks can buffer [`UndoRedo`] implementors via [`&mut World`](World)/[`DeferredWorld`] instead.
///
/// Do not remove or overwrite this resource.
// uses a VecDeque so `CommandsLog` can use `VecDeque::append`
#[derive(Resource, Default)]
pub struct UndoRedoBuffer(VecDeque<SyncCell<Box<dyn UndoRedo>>>);

impl BuffersUndoRedo for UndoRedoBuffer {
    fn buffer_undo_redo(&mut self, undo_redo: impl UndoRedo) {
        self.0.push_back(SyncCell::new(Box::new(undo_redo)));
    }
}

impl UndoRedoBuffer {
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

#[derive(Copy, Clone, Debug, PartialEq)]
pub enum UndoRedoDirection {
    Undo,
    Redo,
    FinalizeUndone,
    FinalizeRedone,
}

pub trait UndoRedo: Send + 'static {
    fn undo(&mut self, world: &mut World);
    fn redo(&mut self, world: &mut World);
    fn finalize_undone(self: Box<Self>, _world: &mut World) {}
    fn finalize_redone(self: Box<Self>, _world: &mut World) {}
}

impl<F: FnMut(&mut World, UndoRedoDirection) + Send + 'static> UndoRedo for F {
    fn undo(&mut self, world: &mut World) {
        self(world, UndoRedoDirection::Undo)
    }
    fn redo(&mut self, world: &mut World) {
        self(world, UndoRedoDirection::Redo)
    }
    fn finalize_undone(mut self: Box<Self>, world: &mut World) {
        self(world, UndoRedoDirection::FinalizeUndone)
    }
    fn finalize_redone(mut self: Box<Self>, world: &mut World) {
        self(world, UndoRedoDirection::FinalizeRedone)
    }
}

#[derive(Default)]
pub(crate) struct UndoRedoLog {
    undo_redo_log: DenseTransitionsLog<SyncCell<Box<dyn UndoRedo>>>,
    frame_log: FrameTransitionLog,
}

#[derive(Debug)]
#[allow(dead_code)]
pub(crate) enum UndoRedoLogErr {
    RevMetaMissing,
    UndoRedoBufferMissing(RevMeta),
    RevDirectionMismatch(RevMeta),
    UnexpectedUpdate(RevMeta),
    OutOfLog(RevMeta),
}

impl UndoRedoLog {
    pub(crate) fn forward(&mut self, world: &mut World) -> Result<(), UndoRedoLogErr> {
        let meta = world
            .get_resource::<RevMeta>()
            .ok_or(UndoRedoLogErr::RevMetaMissing)?
            .clone();
        match meta.get_direction() {
            Some(RevDirection::NOT_LOG) => {
                for command in self
                    .undo_redo_log
                    .drain_future()
                    .0
                    .rev()
                    .map(SyncCell::to_inner)
                {
                    command.finalize_undone(world);
                }
                let max_past_len = self.frame_log.push_and_get_past_len(&meta);
                let mut buffer = world
                    .get_resource_mut::<UndoRedoBuffer>()
                    .ok_or_else(|| UndoRedoLogErr::UndoRedoBufferMissing(meta))?;
                let past_drain = self
                    .undo_redo_log
                    .push_and_drain_past(max_past_len, |mut log| log.append(&mut buffer.0));
                for command in past_drain.0.map(SyncCell::to_inner) {
                    command.finalize_redone(world);
                }
            }
            Some(RevDirection::FORWARD_LOG) => {
                if !self.frame_log.forward_log(&meta) {
                    return Err(UndoRedoLogErr::UnexpectedUpdate(meta));
                };
                let iter = self
                    .undo_redo_log
                    .forward_log()
                    .map_err(|_| UndoRedoLogErr::OutOfLog(meta))?;
                for command in iter.value.map(SyncCell::get) {
                    command.redo(world);
                }
            }
            _ => return Err(UndoRedoLogErr::RevDirectionMismatch(meta)),
        }
        Ok(())
    }
    pub(crate) fn backward(&mut self, world: &mut World) -> Result<(), UndoRedoLogErr> {
        let meta = world
            .get_resource::<RevMeta>()
            .ok_or(UndoRedoLogErr::RevMetaMissing)?
            .clone();
        if meta.get_direction() != Some(RevDirection::BackwardLog) {
            return Err(UndoRedoLogErr::RevDirectionMismatch(meta));
        }
        if !self.frame_log.backward_log(&meta) {
            return Err(UndoRedoLogErr::UnexpectedUpdate(meta));
        };
        let iter = self
            .undo_redo_log
            .backward_log()
            .map_err(|_| UndoRedoLogErr::OutOfLog(meta))?;
        for command in iter.value.rev().map(SyncCell::get) {
            command.undo(world);
        }
        Ok(())
    }
}

use std::{collections::VecDeque, fmt::Debug};

use bevy::{
    ecs::{
        system::Resource,
        world::{DeferredWorld, World},
    },
    prelude::Commands,
    utils::synccell::SyncCell,
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
pub struct UndoRedoLog(TransitionsLog<SyncCell<Box<dyn UndoRedo>>, RevFrame>);

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
        match meta.get_direction() {
            Some(RevDirection::NOT_LOG) => {
                self.clamp_log(world, &meta);
                let mut buffer = world
                    .get_resource_mut::<UndoRedoBuffer>()
                    .ok_or_else(|| UndoRedoErr::UndoRedoBufferMissing(meta.clone()))?;
                if !buffer.0.is_empty() {
                    self.0.push_present(|mut log| {
                        log.append(&mut buffer.0);
                        meta.present_world_state()
                    });
                }
                Ok(())
            }
            Some(RevDirection::FORWARD_LOG) => {
                for command in self
                    .0
                    .forward_log()
                    .map_err(|OutOfLog| UndoRedoErr::OutOfLog(meta))?
                    .into_iter()
                    .map(SyncCell::get)
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
        for command in self
            .0
            .backward_log()
            .map_err(|OutOfLog| UndoRedoErr::OutOfLog(meta.clone()))?
            .into_iter()
            .rev()
            .map(SyncCell::get)
        {
            command.undo(world);
        }
        Ok(())
    }
    pub fn clamp_log(&mut self, world: &mut World, meta: &RevMeta) {
        for command in self.0.drain_future().0.rev().map(SyncCell::to_inner) {
            command.finalize_undone(world);
        }
        // should this be reversed too? recent commands may rely on side effects of older commands that are affected here
        for command in self
            .0
            .drain_past_by_logged_at(&meta)
            .map(SyncCell::to_inner)
        {
            command.finalize_redone(world);
        }
    }
}

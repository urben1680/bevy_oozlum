use std::{
    collections::VecDeque,
    error::Error,
    fmt::{Debug, Display},
};

use bevy::{
    ecs::{
        resource::Resource,
        system::{Commands, EntityCommands},
        world::{DeferredWorld, EntityWorldMut, World},
    },
    utils::synccell::SyncCell,
};

use crate::{
    log::{DenseTransitionsLog, FrameTransitionLog, OutOfLog, SparseTransitionsLog},
    meta::{RevDirection, RevMeta},
};

mod bundle_buffer;
mod commands;

pub use commands::*;

// todo rename
pub trait BuffersRev {
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
    /// | [`EntityWorldMut`] | ✅ | ❌ |
    /// | [`DeferredWorld`] | ✅ | ❌ |
    /// | [`UndoRedoBuffer`] | ✅ | ❌ |
    /// | [`Commands`] | ❌ | ✅ |
    /// | [`EntityCommands`] | ❌ | ✅ |
    fn buffer_undo_redo(&mut self, undo_redo: impl UndoRedo) -> &mut Self;
    fn buffer_finalize(&mut self, finalize: impl Finalize) -> &mut Self;
    fn buffer_undo_redo_finalize(
        &mut self,
        undo_redo_finalize: impl UndoRedo + Finalize + Clone,
    ) -> &mut Self {
        self.buffer_undo_redo(undo_redo_finalize.clone())
            .buffer_finalize(undo_redo_finalize)
    }
}

impl BuffersRev for Commands<'_, '_> {
    fn buffer_undo_redo(&mut self, undo_redo: impl UndoRedo) -> &mut Self {
        self.queue(move |world: &mut World| {
            world.buffer_undo_redo(undo_redo);
        });
        self
    }
    fn buffer_finalize(&mut self, finalize: impl Finalize) -> &mut Self {
        self.queue(move |world: &mut World| {
            world.buffer_finalize(finalize);
        });
        self
    }
}

impl BuffersRev for EntityCommands<'_> {
    fn buffer_undo_redo(&mut self, undo_redo: impl UndoRedo) -> &mut Self {
        self.queue(move |mut world: EntityWorldMut| {
            world.buffer_undo_redo(undo_redo);
        });
        self
    }
    fn buffer_finalize(&mut self, finalize: impl Finalize) -> &mut Self {
        self.queue(move |mut world: EntityWorldMut| {
            world.buffer_finalize(finalize);
        });
        self
    }
}

impl BuffersRev for World {
    fn buffer_undo_redo(&mut self, undo_redo: impl UndoRedo) -> &mut Self {
        DeferredWorld::buffer_undo_redo(&mut self.into(), undo_redo);
        self
    }
    fn buffer_finalize(&mut self, finalize: impl Finalize) -> &mut Self {
        DeferredWorld::buffer_finalize(&mut self.into(), finalize);
        self
    }
}

impl BuffersRev for EntityWorldMut<'_> {
    fn buffer_undo_redo(&mut self, undo_redo: impl UndoRedo) -> &mut Self {
        self.get_resource_mut::<RevBuffer>()
            .expect(EXPECT_BUFFER)
            .buffer_undo_redo(undo_redo);
        self
    }
    fn buffer_finalize(&mut self, finalize: impl Finalize) -> &mut Self {
        self.get_resource_mut::<RevBuffer>()
            .expect(EXPECT_BUFFER)
            .buffer_finalize(finalize);
        self
    }
}

impl BuffersRev for DeferredWorld<'_> {
    fn buffer_undo_redo(&mut self, undo_redo: impl UndoRedo) -> &mut Self {
        self.get_resource_mut::<RevBuffer>()
            .expect(EXPECT_BUFFER)
            .buffer_undo_redo(undo_redo);
        self
    }
    fn buffer_finalize(&mut self, finalize: impl Finalize) -> &mut Self {
        self.get_resource_mut::<RevBuffer>()
            .expect(EXPECT_BUFFER)
            .buffer_finalize(finalize);
        self
    }
}

impl BuffersRev for RevBuffer {
    fn buffer_undo_redo(&mut self, undo_redo: impl UndoRedo) -> &mut Self {
        self.undo_redo_buffer
            .push_back(SyncCell::new(Box::new(undo_redo)));
        self
    }
    fn buffer_finalize(&mut self, finalize: impl Finalize) -> &mut Self {
        self.finalize_buffer
            .push_back(SyncCell::new(Box::new(finalize)));
        self
    }
}

const EXPECT_BUFFER: &'static str =
    "BuffersUndoRedo methods need the UndoRedoBuffer resource but it is missing";

/// For usages in reversible observer systems.
///
/// Commands and hooks can buffer [`UndoRedo`] implementors via [`&mut World`](World)/[`DeferredWorld`] instead.
///
/// Do not remove or overwrite this resource.
// uses a VecDeque so `CommandsLog` can use `VecDeque::append`
#[derive(Resource, Default)]
pub struct RevBuffer {
    undo_redo_buffer: VecDeque<SyncCell<Box<dyn UndoRedo>>>,
    finalize_buffer: VecDeque<SyncCell<Box<dyn Finalize>>>,
    finalize_log: SparseTransitionsLog<SyncCell<Box<dyn Finalize>>>,
}

impl RevBuffer {
    pub fn undo_redo_is_empty(&self) -> bool {
        self.undo_redo_buffer.is_empty()
    }
    pub(crate) fn finish_rev_update(
        &mut self,
        meta: &RevMeta,
        world: &mut World,
    ) -> Result<(), OutOfLog> {
        match meta.get_direction() {
            None => return Ok(()),
            Some(RevDirection::NOT_LOG) => {
                for finalize in self
                    .finalize_log
                    .drain_future()
                    .0
                    .rev()
                    .map(SyncCell::to_inner)
                {
                    finalize.finalize_undone(world);
                }
                let past_len = meta.past_len() as usize + 1;
                let past_drain = if self.finalize_buffer.is_empty() {
                    self.finalize_log.push_none_and_drain_past(past_len)
                } else {
                    self.finalize_log
                        .push_some_and_drain_past(past_len, |mut log| {
                            log.append(&mut self.finalize_buffer);
                        })
                };
                for finalize in past_drain.0.map(SyncCell::to_inner) {
                    finalize.finalize_redone(world);
                }
                Ok(())
            }
            Some(RevDirection::FORWARD_LOG) => self.finalize_log.forward_log().map(|_| ()),
            Some(RevDirection::BackwardLog) => self.finalize_log.backward_log().map(|_| ()),
        }
    }
}

pub trait UndoRedo: Send + 'static {
    fn undo(&mut self, world: &mut World);
    fn redo(&mut self, world: &mut World);
}

pub trait Finalize: Send + 'static {
    fn finalize_undone(self: Box<Self>, world: &mut World);
    fn finalize_redone(self: Box<Self>, world: &mut World);
}

#[derive(Copy, Clone, Debug, PartialEq)]
pub enum UndoRedoDirection {
    Undo,
    Redo,
}

#[derive(Copy, Clone, Debug, PartialEq)]
pub enum FinalizeDirection {
    FinalizeUndone,
    FinalizeRedone,
}

impl<F: FnMut(&mut World, UndoRedoDirection) + Send + 'static> UndoRedo for F {
    fn undo(&mut self, world: &mut World) {
        self(world, UndoRedoDirection::Undo)
    }
    fn redo(&mut self, world: &mut World) {
        self(world, UndoRedoDirection::Redo)
    }
}

impl<T: UndoRedo> UndoRedo for Vec<T> {
    fn undo(&mut self, world: &mut World) {
        for x in self.iter_mut().rev() {
            x.undo(world);
        }
    }
    fn redo(&mut self, world: &mut World) {
        for x in self.iter_mut() {
            x.undo(world);
        }
    }
}

impl<T: UndoRedo, const N: usize> UndoRedo for [T; N] {
    fn undo(&mut self, world: &mut World) {
        for x in self.iter_mut().rev() {
            x.undo(world);
        }
    }
    fn redo(&mut self, world: &mut World) {
        for x in self.iter_mut() {
            x.undo(world);
        }
    }
}

impl<T: UndoRedo> UndoRedo for [T] {
    fn undo(&mut self, world: &mut World) {
        for x in self.iter_mut().rev() {
            x.undo(world);
        }
    }
    fn redo(&mut self, world: &mut World) {
        for x in self.iter_mut() {
            x.undo(world);
        }
    }
}

impl<F: FnMut(&mut World, FinalizeDirection) + Send + 'static> Finalize for F {
    fn finalize_undone(mut self: Box<Self>, world: &mut World) {
        self(world, FinalizeDirection::FinalizeUndone)
    }
    fn finalize_redone(mut self: Box<Self>, world: &mut World) {
        self(world, FinalizeDirection::FinalizeRedone)
    }
}

impl<T: Finalize> Finalize for Vec<T> {
    fn finalize_undone(self: Box<Self>, world: &mut World) {
        for x in self.into_iter().rev().map(Box::new) {
            x.finalize_undone(world);
        }
    }
    fn finalize_redone(self: Box<Self>, world: &mut World) {
        for x in self.into_iter().map(Box::new) {
            x.finalize_redone(world);
        }
    }
}

impl<T: Finalize, const N: usize> Finalize for [T; N] {
    fn finalize_undone(self: Box<Self>, world: &mut World) {
        for x in self.into_iter().rev().map(Box::new) {
            x.finalize_undone(world);
        }
    }
    fn finalize_redone(self: Box<Self>, world: &mut World) {
        for x in self.into_iter().map(Box::new) {
            x.finalize_redone(world);
        }
    }
}

impl<T: Finalize> Finalize for [T] {
    fn finalize_undone(self: Box<Self>, world: &mut World) {
        for x in IntoIterator::into_iter(self).rev().map(Box::new) {
            x.finalize_undone(world);
        }
    }
    fn finalize_redone(self: Box<Self>, world: &mut World) {
        for x in IntoIterator::into_iter(self).map(Box::new) {
            x.finalize_redone(world);
        }
    }
}

#[derive(Default)]
pub(crate) struct UndoRedoLog {
    undo_redo_log: DenseTransitionsLog<SyncCell<Box<dyn UndoRedo>>>,
    frame_log: FrameTransitionLog,
}

#[derive(Debug)]
#[allow(dead_code)]
pub(crate) enum UndoRedoLogError<'a> {
    RevMetaMissing { system_name: &'a str },
    UndoRedoBufferMissing { now: u64, system_name: &'a str },
    RevDirectionMismatch { now: u64, system_name: &'a str },
    OutOfLog { now: u64, system_name: &'a str },
}

impl UndoRedoLog {
    pub(crate) fn forward<'a>(
        &mut self,
        world: &mut World,
        system_name: &'a str,
    ) -> Result<(), UndoRedoLogError<'a>> {
        let meta = world
            .get_resource::<RevMeta>()
            .ok_or(UndoRedoLogError::RevMetaMissing { system_name })?
            .clone();
        let now = meta.now();
        match meta.get_direction() {
            Some(RevDirection::NOT_LOG) => {
                let mut buffer = world
                    .get_resource_mut::<RevBuffer>()
                    .ok_or_else(|| UndoRedoLogError::UndoRedoBufferMissing { now, system_name })?;
                if !buffer.undo_redo_is_empty() {
                    let past_len = self.frame_log.push_and_get_past_len(&meta);
                    self.undo_redo_log.push_and_drain_past(past_len, |mut log| {
                        log.append(&mut buffer.undo_redo_buffer)
                    });
                }
            }
            Some(RevDirection::FORWARD_LOG) => {
                if !self.frame_log.forward_log(&meta) {
                    return Ok(());
                };
                let iter = self
                    .undo_redo_log
                    .forward_log()
                    .map_err(|_| UndoRedoLogError::OutOfLog { now, system_name })?
                    .value
                    .map(SyncCell::get);
                for command in iter {
                    command.redo(world);
                }
            }
            _ => return Err(UndoRedoLogError::RevDirectionMismatch { now, system_name }),
        }
        Ok(())
    }
    pub(crate) fn backward<'a>(
        &mut self,
        world: &mut World,
        system_name: &'a str,
    ) -> Result<(), UndoRedoLogError<'a>> {
        let meta = world
            .get_resource::<RevMeta>()
            .ok_or(UndoRedoLogError::RevMetaMissing { system_name })?
            .clone();
        let now = meta.now();
        if meta.get_direction() != Some(RevDirection::BackwardLog) {
            return Err(UndoRedoLogError::RevDirectionMismatch { now, system_name });
        }
        if !self.frame_log.backward_log(&meta) {
            return Ok(());
        };
        let iter = self
            .undo_redo_log
            .backward_log()
            .map_err(|_| UndoRedoLogError::OutOfLog { now, system_name })?
            .value
            .map(SyncCell::get)
            .rev();
        for command in iter {
            command.undo(world);
        }
        Ok(())
    }
}

impl<'a> Display for UndoRedoLogError<'a> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::RevMetaMissing { system_name } => write!(f, "RevMeta was removed but is needed to update the UndoRedo log of reversible system {system_name}"),
            Self::UndoRedoBufferMissing { now, system_name } => write!(f, "UndoRedoBuffer was removed at frame {now} but is needed to update the UndoRedo log of reversible system {system_name}"),
            Self::RevDirectionMismatch { now, system_name } => write!(f, "RevDirection changed to an incorrect value at frame {now} before the update of the UndoRedo log of reversible system {system_name}"),
            Self::OutOfLog { now, system_name } => write!(f, "the UndoRedo log of the reversible system {system_name} is in an invalid state at frame {now}"),
        }
    }
}

impl<'a> Error for UndoRedoLogError<'a> {}

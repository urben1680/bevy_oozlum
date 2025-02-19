use std::{
    any::TypeId,
    collections::VecDeque,
    error::Error,
    fmt::{Debug, Display},
};

use bevy::{
    ecs::{
        archetype::Archetype,
        bundle::{Bundle, BundleId, BundleInfo},
        component::{Component, ComponentId},
        entity::Entity,
        resource::Resource,
        system::{Commands, EntityCommands},
        world::{DeferredWorld, EntityWorldMut, FromWorld, World},
    },
    utils::synccell::SyncCell,
};

use crate::{
    log::{DenseTransitionsLog, FrameTransitionLog, OutOfLog, SparseTransitionsLog},
    meta::{RevDirection, RevMeta},
};

mod commands;
mod entity_commands;

pub use commands::*;
pub use entity_commands::*;

// todo rename
pub trait BuffersUndoRedoFinalize {
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

impl BuffersUndoRedoFinalize for Commands<'_, '_> {
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

impl BuffersUndoRedoFinalize for EntityCommands<'_> {
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

impl BuffersUndoRedoFinalize for World {
    fn buffer_undo_redo(&mut self, undo_redo: impl UndoRedo) -> &mut Self {
        DeferredWorld::buffer_undo_redo(&mut self.into(), undo_redo);
        self
    }
    fn buffer_finalize(&mut self, finalize: impl Finalize) -> &mut Self {
        DeferredWorld::buffer_finalize(&mut self.into(), finalize);
        self
    }
}

impl BuffersUndoRedoFinalize for EntityWorldMut<'_> {
    fn buffer_undo_redo(&mut self, undo_redo: impl UndoRedo) -> &mut Self {
        self.get_resource_mut::<RevBuffers>()
            .expect(EXPECT_BUFFER)
            .buffer_undo_redo(undo_redo);
        self
    }
    fn buffer_finalize(&mut self, finalize: impl Finalize) -> &mut Self {
        self.get_resource_mut::<RevBuffers>()
            .expect(EXPECT_BUFFER)
            .buffer_finalize(finalize);
        self
    }
}

impl BuffersUndoRedoFinalize for DeferredWorld<'_> {
    fn buffer_undo_redo(&mut self, undo_redo: impl UndoRedo) -> &mut Self {
        self.get_resource_mut::<RevBuffers>()
            .expect(EXPECT_BUFFER)
            .buffer_undo_redo(undo_redo);
        self
    }
    fn buffer_finalize(&mut self, finalize: impl Finalize) -> &mut Self {
        self.get_resource_mut::<RevBuffers>()
            .expect(EXPECT_BUFFER)
            .buffer_finalize(finalize);
        self
    }
}

impl BuffersUndoRedoFinalize for RevBuffers {
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
pub struct RevBuffers {
    undo_redo_buffer: VecDeque<SyncCell<Box<dyn UndoRedo>>>,
    finalize_buffer: VecDeque<SyncCell<Box<dyn Finalize>>>,
    finalize_log: SparseTransitionsLog<SyncCell<Box<dyn Finalize>>>,
}

impl RevBuffers {
    pub fn undo_redo_is_empty(&self) -> bool {
        self.undo_redo_buffer.is_empty()
    }
    pub fn finalize_all(mut self, world: &mut World) {
        self.update_finalize(world, 0);
    }
    pub(crate) fn finish_rev_update(
        &mut self,
        meta: &RevMeta,
        world: &mut World,
    ) -> Result<(), OutOfLog> {
        match meta.get_direction() {
            None => return Ok(()),
            Some(RevDirection::NOT_LOG) => {
                self.update_finalize(world, meta.past_len() as usize + 1);
                Ok(())
            }
            Some(RevDirection::FORWARD_LOG) => self.finalize_log.forward_log().map(|_| ()),
            Some(RevDirection::BackwardLog) => self.finalize_log.backward_log().map(|_| ()),
        }
    }
    fn update_finalize(&mut self, world: &mut World, past_len: usize) {
        let future_drain = self
            .finalize_log
            .drain_future()
            .0
            .rev()
            .map(SyncCell::to_inner);
        for finalize in future_drain {
            finalize.finalize_undone(world);
        }
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
    }
    #[cfg(test)]
    pub fn pop_undo_redo(&mut self) -> Option<Box<dyn UndoRedo>> {
        self.undo_redo_buffer.pop_back().map(SyncCell::to_inner)
    }
    #[cfg(test)]
    pub fn pop_finalize(&mut self) -> Option<Box<dyn Finalize>> {
        self.finalize_buffer.pop_back().map(SyncCell::to_inner)
    }
}

/// Marker component that disables entities like [`Disabled`].
///
/// Entities marked with this component will be despawned when the relevant reversible command, for example
/// an undone [`rev_spawn_batch`], is in the state that added this marker and will be finalized by either the
/// [`RevMeta::run_rev_update`] system or [`RevBuffers::finalize_all`].
///
/// Until then entities with this marker should be considered as non-existing.
#[derive(Component)]
pub struct RevDisabled;

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

impl<T: UndoRedo> UndoRedo for Box<[T]> {
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
                    .get_resource_mut::<RevBuffers>()
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

/// Get a scope of the world and an entity id that is empty.
///
/// When the closure returns, the entity is cleared from any remaining components.
///
/// The entity may be changed, for example when components are moved into it and another
/// entity became empty. One should not otherwise put any assumptions on the entity
/// after returning as other code calling this method may change the empty entity as well.
pub fn empty_entity_scope<Out>(
    world: &mut World,
    c: impl FnOnce(&mut World, &mut Entity) -> Out,
) -> Out {
    #[derive(Resource)]
    struct EmptyEntity(Entity);

    impl FromWorld for EmptyEntity {
        fn from_world(world: &mut World) -> Self {
            // do not just take any existing empty entity, it likely belongs to other logic
            Self(world.spawn_empty().id())
        }
    }

    world.init_resource::<EmptyEntity>();
    world.resource_scope::<EmptyEntity, Out>(|world, mut entity| {
        let out = c(world, &mut entity.0);
        world.entity_mut(entity.0).clear();
        out
    })
}

fn archetype_insert_if_new(bundle_info: &BundleInfo, archetype: &Archetype) -> Box<[ComponentId]> {
    bundle_info
        .iter_contributed_components()
        .filter(|component_id| !archetype.contains(*component_id))
        .collect()
}

fn archetype_insert_replace(bundle_info: &BundleInfo, archetype: &Archetype) -> Box<[ComponentId]> {
    bundle_info
        .iter_required_components()
        .filter(|component_id| !archetype.contains(*component_id))
        .chain(bundle_info.iter_explicit_components())
        .collect()
}

fn archetype_insert_replace_backup(
    bundle_info: &BundleInfo,
    archetype: &Archetype,
) -> Box<[ComponentId]> {
    bundle_info
        .iter_explicit_components()
        .filter(|component_id| archetype.contains(*component_id))
        .collect()
}

/// todo workaround until manual bundle registration is possible
fn get_bundle_id<T: Bundle>(world: &mut World) -> BundleId {
    empty_entity_scope(world, |world, empty_entity| {
        let type_id = TypeId::of::<T>();
        if let Some(id) = world.bundles().get_id(type_id) {
            return id;
        }
        world.entity_mut(*empty_entity).remove::<T>();
        world
            .bundles()
            .get_id(type_id)
            .expect("above command should have registered bundle")
    })
}

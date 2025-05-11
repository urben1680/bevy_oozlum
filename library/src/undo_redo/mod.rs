use std::{
    any::{type_name, type_name_of_val},
    error::Error,
    fmt::{Debug, Display},
    hash::Hash,
    iter::FusedIterator,
    marker::PhantomData,
    sync::Arc,
};

use bevy::{
    ecs::{
        archetype::ArchetypeId,
        bundle::{Bundle, BundleEffect, BundleFromComponents, BundleId, DynamicBundle, InsertMode},
        change_detection::{MaybeLocation, Mut},
        component::{
            Component, ComponentCloneBehavior, ComponentId, ComponentMutability, Components,
            ComponentsRegistrator, RequiredComponents, StorageType,
        },
        entity::{Entity, EntityCloner, EntityDoesNotExistError, hash_set::EntityHashSet},
        hierarchy::{ChildOf, Children},
        relationship::{OrderedRelationshipSourceCollection, Relationship, RelationshipTarget},
        resource::Resource,
        system::{Commands, EntityCommands},
        world::{
            DeferredWorld, EntityMut, EntityMutExcept, EntityRef, EntityRefExcept, EntityWorldMut,
            FilteredEntityMut, FilteredEntityRef, World, error::EntityMutableFetchError,
        },
    },
    log::{error, warn},
    platform::collections::{HashMap, HashSet},
    ptr::OwningPtr,
    utils::synccell::SyncCell,
};

use crate::{
    app::RevSystemsPlugin,
    log::{DenseTransitionsLog, FrameTransitionLog, MissedFrame},
    meta::{RevDirection, RevMeta},
};

mod bundle_buffer;
//mod commands;
mod spawn_despawn;
//mod entity_commands;
mod entity_world;
mod world;

pub use bundle_buffer::*;
//pub use commands::*;
pub use spawn_despawn::*;
//pub use entity_commands::*;
pub use entity_world::*;
pub use world::*;

#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub struct RevMetaMissing;

impl Display for RevMetaMissing {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", RevMeta::EXPECT_IN_WORLD)
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub struct RevDirectionMismatch {
    pub actual: Option<RevDirection>,
}

impl Display for RevDirectionMismatch {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "expected RevMeta in the `Some(RevDirection::NOT_LOG)` running direction but was in `{:?}`",
            self.actual
        )
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub enum RevMetaNotLogError {
    RevMetaMissing(RevMetaMissing),
    RevDirectionMismatch(RevDirectionMismatch),
}

impl From<RevMetaMissing> for RevMetaNotLogError {
    fn from(value: RevMetaMissing) -> Self {
        Self::RevMetaMissing(value)
    }
}

impl From<RevDirectionMismatch> for RevMetaNotLogError {
    fn from(value: RevDirectionMismatch) -> Self {
        Self::RevDirectionMismatch(value)
    }
}

impl Display for RevMetaNotLogError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::RevMetaMissing(inner) => write!(f, "{inner}"),
            Self::RevDirectionMismatch(inner) => write!(f, "{inner}"),
        }
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct EntityRevDespawnedError {
    pub entity: Entity,
    pub marker: DisabledToDespawn,
}

impl Display for EntityRevDespawnedError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.marker.added_location().into_option() {
            None => write!(
                f,
                "entity {} is marked as reversibly despawned (enable `track_location` feature for more details)",
                self.entity
            ),
            Some(Some(location)) => write!(
                f,
                "entity {} is marked as reversibly despawned by {location}",
                self.entity
            ),
            Some(None) => write!(
                f,
                "entity {} is a bundle buffer which is not intended to be manually modified",
                self.entity
            ),
        }
    }
}

impl Error for EntityRevDespawnedError {}

impl EntityRevDespawnedError {
    pub(super) fn new(entity: Entity, marker: DisabledToDespawn) -> Self {
        EntityRevDespawnedError { entity, marker }
    }
}

impl Error for RevMetaNotLogError {}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum RevEntityError {
    EntityDoesNotExistError(EntityDoesNotExistError),
    EntityRevDespawnedError(EntityRevDespawnedError),
}

impl From<EntityDoesNotExistError> for RevEntityError {
    fn from(value: EntityDoesNotExistError) -> Self {
        Self::EntityDoesNotExistError(value)
    }
}

impl From<EntityRevDespawnedError> for RevEntityError {
    fn from(value: EntityRevDespawnedError) -> Self {
        Self::EntityRevDespawnedError(value)
    }
}

impl Display for RevEntityError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EntityDoesNotExistError(inner) => write!(f, "{inner}"),
            Self::EntityRevDespawnedError(inner) => write!(f, "{inner}"),
        }
    }
}

impl Error for RevEntityError {}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum RevMetaOrEntityError {
    RevMetaNotLogError(RevMetaNotLogError),
    RevEntityError(RevEntityError),
}

impl From<RevMetaMissing> for RevMetaOrEntityError {
    fn from(value: RevMetaMissing) -> Self {
        Self::RevMetaNotLogError(value.into())
    }
}

impl From<RevDirectionMismatch> for RevMetaOrEntityError {
    fn from(value: RevDirectionMismatch) -> Self {
        Self::RevMetaNotLogError(value.into())
    }
}

impl From<RevMetaNotLogError> for RevMetaOrEntityError {
    fn from(value: RevMetaNotLogError) -> Self {
        Self::RevMetaNotLogError(value)
    }
}

impl From<EntityDoesNotExistError> for RevMetaOrEntityError {
    fn from(value: EntityDoesNotExistError) -> Self {
        Self::RevEntityError(value.into())
    }
}

impl From<EntityRevDespawnedError> for RevMetaOrEntityError {
    fn from(value: EntityRevDespawnedError) -> Self {
        Self::RevEntityError(value.into())
    }
}

impl Display for RevMetaOrEntityError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::RevMetaNotLogError(inner) => write!(f, "{inner}"),
            Self::RevEntityError(inner) => write!(f, "{inner}"),
        }
    }
}

impl Error for RevMetaOrEntityError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RevEntitiesError {
    pub invalid: Vec<EntityDoesNotExistError>,
    pub rev_despawned: Vec<EntityRevDespawnedError>,
}

impl Display for RevEntitiesError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if size_of::<MaybeLocation<u8>>() == 0 {
            if !self.invalid.is_empty() {
                write!(f, "non-existing entities: ")?;
                for entity in self.invalid.iter() {
                    write!(f, "{}, ", entity.entity)?;
                }
            }
            if !self.rev_despawned.is_empty() {
                write!(f, "reversibly despawned entities: ")?;
                for entity in self.rev_despawned.iter() {
                    write!(f, "{}, ", entity.entity)?;
                }
            }
            write!(f, "(enable `track_location` feature for more details)")?;
        } else {
            let invalid = self.invalid.iter().map(|err| err.entity);
            let rev_despawned = self.rev_despawned.iter().map(|err| err.entity);
            for entity in invalid.chain(rev_despawned) {
                write!(f, "{entity}, ")?;
            }
        }
        Ok(())
    }
}

impl Error for RevEntitiesError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RevMetaOrEntitiesError {
    RevMetaNotLogError(RevMetaNotLogError),
    RevEntitiesError(RevEntitiesError),
}

impl From<RevMetaMissing> for RevMetaOrEntitiesError {
    fn from(value: RevMetaMissing) -> Self {
        Self::RevMetaNotLogError(value.into())
    }
}

impl From<RevDirectionMismatch> for RevMetaOrEntitiesError {
    fn from(value: RevDirectionMismatch) -> Self {
        Self::RevMetaNotLogError(value.into())
    }
}

impl From<RevMetaNotLogError> for RevMetaOrEntitiesError {
    fn from(value: RevMetaNotLogError) -> Self {
        Self::RevMetaNotLogError(value)
    }
}

impl Display for RevMetaOrEntitiesError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::RevMetaNotLogError(inner) => write!(f, "{inner}"),
            Self::RevEntitiesError(inner) => write!(f, "{inner}"),
        }
    }
}

impl Error for RevMetaOrEntitiesError {}

pub trait BuffersUndoRedo {
    /// Buffers an [`UndoRedo`] implementor in a resource to be collected by the reversible system's state during sync points.
    ///
    /// Logic applied in sync points are in:
    /// - commands
    /// - hooks
    /// - observers
    /// - bundle effects
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
}

// todo: replace all following with Rev* wrappers OR integrate in wrapper impls
// how to support DeferredWorld in hooks/observers?

impl BuffersUndoRedo for Commands<'_, '_> {
    fn buffer_undo_redo(&mut self, undo_redo: impl UndoRedo) -> &mut Self {
        self.queue(move |world: &mut World| {
            world.buffer_undo_redo(undo_redo);
        });
        self
    }
}

impl BuffersUndoRedo for EntityCommands<'_> {
    fn buffer_undo_redo(&mut self, undo_redo: impl UndoRedo) -> &mut Self {
        self.queue(move |mut world: EntityWorldMut| {
            world.buffer_undo_redo(undo_redo);
        });
        self
    }
}

impl BuffersUndoRedo for World {
    fn buffer_undo_redo(&mut self, undo_redo: impl UndoRedo) -> &mut Self {
        DeferredWorld::buffer_undo_redo(&mut self.into(), undo_redo);
        self
    }
}

impl BuffersUndoRedo for EntityWorldMut<'_> {
    fn buffer_undo_redo(&mut self, undo_redo: impl UndoRedo) -> &mut Self {
        self.get_resource_mut::<UndoRedoBuffer>()
            .expect(UndoRedoBuffer::EXPECT_IN_WORLD)
            .buffer_undo_redo(undo_redo);
        self
    }
}

impl BuffersUndoRedo for DeferredWorld<'_> {
    fn buffer_undo_redo(&mut self, undo_redo: impl UndoRedo) -> &mut Self {
        self.get_resource_mut::<UndoRedoBuffer>()
            .expect(UndoRedoBuffer::EXPECT_IN_WORLD)
            .buffer_undo_redo(undo_redo);
        self
    }
}

impl BuffersUndoRedo for UndoRedoBuffer {
    fn buffer_undo_redo(&mut self, undo_redo: impl UndoRedo) -> &mut Self {
        let name = type_name_of_val(&undo_redo);
        let boxed = BoxedUndoRedo {
            undo_redo: SyncCell::new(Box::new(undo_redo)),
            name,
        };
        self.0.push(boxed);
        self
    }
}

/// For usages in _observer_ systems. Regular reversible systems should use commands or &mut World.
///
/// Commands and hooks can buffer [`UndoRedo`] implementors via [`&mut World`](World)/[`DeferredWorld`] instead.
///
/// Do not remove or overwrite this resource.
#[derive(Resource, Default, Debug)]
pub struct UndoRedoBuffer(Vec<BoxedUndoRedo>);

impl UndoRedoBuffer {
    pub(crate) const EXPECT_IN_WORLD: &'static str =
        "BuffersUndoRedo methods need the UndoRedoBuffer resource but it is missing";
    /// Returns `true` when the buffer is empty, otherwise returns `false`.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn type_names(&self) -> impl ExactSizeIterator<Item = &'static str> + '_ {
        self.0.iter().map(|boxed| boxed.name)
    }
}

#[cfg(test)]
impl UndoRedo for UndoRedoBuffer {
    fn undo(&mut self, world: &mut World) {
        for boxed in self.0.iter_mut().rev() {
            boxed.undo_redo.get().undo(world)
        }
    }
    fn redo(&mut self, world: &mut World) {
        for boxed in self.0.iter_mut() {
            boxed.undo_redo.get().redo(world)
        }
    }
}

struct BoxedUndoRedo {
    undo_redo: SyncCell<Box<dyn UndoRedo>>,
    name: &'static str,
}

impl Debug for BoxedUndoRedo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.name)
    }
}

pub trait UndoRedo: Send + 'static {
    fn undo(&mut self, world: &mut World);
    fn redo(&mut self, world: &mut World);
}

#[derive(Copy, Clone, Debug, PartialEq)]
pub enum UndoRedoDirection {
    Undo,
    Redo,
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

#[derive(Default)]
pub(crate) struct UndoRedoLog {
    undo_redo_log: DenseTransitionsLog<SyncCell<Box<dyn UndoRedo>>>,
    frame_log: FrameTransitionLog,
}

#[derive(Debug)]
pub(crate) enum UndoRedoLogError {
    RevMetaMissing {
        system_name: String,
    },
    UndoRedoBufferMissing {
        now: u64,
        system_name: String,
    },
    RevDirectionMismatch {
        now: u64,
        system_name: String,
    },
    MissedFrame {
        frame: u64,
        now: u64,
        system_name: String,
    },
    OutOfLog {
        now: u64,
        system_name: String,
    },
}

impl UndoRedoLog {
    pub(crate) fn forward(
        &mut self,
        world: &mut World,
        system_name: &str,
    ) -> Result<(), UndoRedoLogError> {
        let meta = world
            .get_resource::<RevMeta>()
            .ok_or_else(|| UndoRedoLogError::RevMetaMissing {
                system_name: system_name.to_owned(),
            })?
            .clone();
        let now = meta.now();
        match meta.get_running_direction() {
            Some(RevDirection::NOT_LOG) => {
                let mut buffer = world.get_resource_mut::<UndoRedoBuffer>().ok_or_else(|| {
                    UndoRedoLogError::UndoRedoBufferMissing {
                        now,
                        system_name: system_name.to_owned(),
                    }
                })?;
                if !buffer.0.is_empty() {
                    let past_len = self.frame_log.push_and_get_past_len(&meta);
                    self.undo_redo_log.push_and_drain_past(past_len, |mut log| {
                        log.extend(buffer.0.drain(..).map(|boxed| boxed.undo_redo))
                    });
                }
                Ok(())
            }
            Some(RevDirection::FORWARD_LOG) => {
                if !self
                    .frame_log
                    .try_forward_log(&meta)
                    .map_err(map_frame_log_err(now, system_name))?
                {
                    return Ok(());
                };
                let iter = self
                    .undo_redo_log
                    .forward_log()
                    .map_err(|_| UndoRedoLogError::OutOfLog {
                        now,
                        system_name: system_name.to_owned(),
                    })?
                    .value
                    .map(SyncCell::get);
                for command in iter {
                    command.redo(world);
                }
                Ok(())
            }
            _ => Err(UndoRedoLogError::RevDirectionMismatch {
                now,
                system_name: system_name.to_owned(),
            }),
        }
    }
    pub(crate) fn backward(
        &mut self,
        world: &mut World,
        system_name: &str,
    ) -> Result<(), UndoRedoLogError> {
        let meta = world
            .get_resource::<RevMeta>()
            .ok_or_else(|| UndoRedoLogError::RevMetaMissing {
                system_name: system_name.to_owned(),
            })?
            .clone();
        let now = meta.now();
        if meta.get_running_direction() != Some(RevDirection::BackwardLog) {
            return Err(UndoRedoLogError::RevDirectionMismatch {
                now,
                system_name: system_name.to_owned(),
            });
        }
        if !self
            .frame_log
            .try_backward_log(&meta)
            .map_err(map_frame_log_err(now, system_name))?
        {
            return Ok(());
        };
        let iter = self
            .undo_redo_log
            .backward_log()
            .map_err(|_| UndoRedoLogError::OutOfLog {
                now,
                system_name: system_name.to_owned(),
            })?
            .value
            .map(SyncCell::get)
            .rev();
        for command in iter {
            command.undo(world);
        }
        Ok(())
    }
}

impl Display for UndoRedoLogError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::RevMetaMissing { system_name } => write!(
                f,
                "RevMeta was removed but is needed to update the UndoRedo log of reversible system {system_name}"
            ),
            Self::UndoRedoBufferMissing { now, system_name } => write!(
                f,
                "UndoRedoBuffer was removed at frame {now} but is needed to update the UndoRedo log of reversible system {system_name}"
            ),
            Self::RevDirectionMismatch { now, system_name } => write!(
                f,
                "RevDirection changed to an incorrect value at frame {now} before the update of the UndoRedo log of reversible system {system_name}"
            ),
            Self::MissedFrame {
                frame,
                now,
                system_name,
            } => write!(
                f,
                "the UndoRedo log of the reversible system {system_name} ran at {now} and missed to run at {frame}"
            ),
            Self::OutOfLog { now, system_name } => write!(
                f,
                "the UndoRedo log of the reversible system {system_name} is in an invalid state at frame {now}"
            ),
        }
    }
}

impl Error for UndoRedoLogError {}

fn map_frame_log_err(
    now: u64,
    system_name: &str,
) -> impl FnOnce(MissedFrame) -> UndoRedoLogError + '_ {
    move |err| UndoRedoLogError::MissedFrame {
        frame: err.0,
        now,
        system_name: system_name.to_owned(),
    }
}

pub struct UndoRedoSwap<T: UndoRedo>(pub T);

impl<T: UndoRedo> UndoRedo for UndoRedoSwap<T> {
    fn undo(&mut self, world: &mut World) {
        self.0.redo(world);
    }
    fn redo(&mut self, world: &mut World) {
        self.0.undo(world);
    }
}

struct InsertRelationship<R, Target>
where
    R: Relationship,
    Target: AsRef<[Entity]> + Send + Sync + 'static,
{
    entity: Entity,
    target: Target,
    _marker: PhantomData<R>,
}

impl<R, Target> UndoRedo for InsertRelationship<R, Target>
where
    R: Relationship,
    Target: AsRef<[Entity]> + Send + Sync + 'static,
{
    fn undo(&mut self, world: &mut World) {
        let component_id = world.component_id::<R>().unwrap();
        for &target in self.target.as_ref().into_iter() {
            world.entity_mut(target).remove_by_id(component_id);
        }
    }
    fn redo(&mut self, world: &mut World) {
        world.insert_batch(
            self.target
                .as_ref()
                .into_iter()
                .copied()
                .map(|target| (target, R::from(self.entity))),
        );
    }
}

struct InsertExistingRelated<R> {
    id: Entity,
    related: Entity,
    index: usize,
    old_index: usize,
    _marker: PhantomData<R>,
}

impl<R> InsertExistingRelated<R>
where
    R: Relationship,
    <R::RelationshipTarget as RelationshipTarget>::Collection: OrderedRelationshipSourceCollection,
{
    fn undo_redo(&self, world: &mut World, index: usize) {
        world
            .get_mut::<R::RelationshipTarget>(self.id)
            .expect("todo")
            .collection_mut_risky()
            .place(self.related, index);
    }
}

impl<R> UndoRedo for InsertExistingRelated<R>
where
    R: Relationship,
    <R::RelationshipTarget as RelationshipTarget>::Collection: OrderedRelationshipSourceCollection,
{
    fn undo(&mut self, world: &mut World) {
        self.undo_redo(world, self.old_index);
    }
    fn redo(&mut self, world: &mut World) {
        self.undo_redo(world, self.index);
    }
}

struct InsertNewRelated<R> {
    id: Entity,
    related: Entity,
    index: usize,
    _marker: PhantomData<R>,
}

impl<R> UndoRedo for InsertNewRelated<R>
where
    R: Relationship,
    <R::RelationshipTarget as RelationshipTarget>::Collection: OrderedRelationshipSourceCollection,
{
    fn undo(&mut self, world: &mut World) {
        world.entity_mut(self.related).remove::<ChildOf>();
    }
    fn redo(&mut self, world: &mut World) {
        world.entity_mut(self.related).insert(R::from(self.id));
        world
            .get_mut::<R::RelationshipTarget>(self.id)
            .expect("todo")
            .collection_mut_risky()
            .place_most_recent(self.index);
    }
}

struct Take<B> {
    bundle: Option<B>,
    entity: Entity,
}

impl<B: Bundle + BundleFromComponents> UndoRedo for Take<B> {
    fn undo(&mut self, world: &mut World) {
        world
            .entity_mut(self.entity)
            .insert(self.bundle.take().expect("todo"));
    }
    fn redo(&mut self, world: &mut World) {
        self.bundle = world.entity_mut(self.entity).take::<B>();
    }
}

struct ResourceSwap<R: Resource>(Option<R>);

impl<R: Resource> UndoRedo for ResourceSwap<R> {
    fn undo(&mut self, world: &mut World) {
        match world.get_resource_mut::<R>() {
            Some(mut r1) => match self.0.as_mut() {
                Some(r2) => core::mem::swap(&mut *r1, r2),
                None => self.0 = world.remove_resource::<R>(),
            },
            None => {
                if let Some(r2) = self.0.take() {
                    world.insert_resource(r2)
                }
            }
        }
    }
    fn redo(&mut self, world: &mut World) {
        self.undo(world)
    }
}

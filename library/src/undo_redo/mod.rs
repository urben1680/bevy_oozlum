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
            Component, ComponentCloneBehavior, ComponentId, Components, ComponentsRegistrator,
            RequiredComponents, StorageType,
        },
        entity::{Entity, EntityCloner, EntityDoesNotExistError, hash_set::EntityHashSet},
        hierarchy::{ChildOf, Children},
        relationship::{OrderedRelationshipSourceCollection, Relationship, RelationshipTarget},
        resource::Resource,
        system::{Commands, EntityCommands},
        world::{
            DeferredWorld, EntityMut, EntityMutExcept, EntityRef, EntityRefExcept, EntityWorldMut,
            FilteredEntityMut, FilteredEntityRef, FromWorld, World, error::EntityMutableFetchError,
        },
    },
    log::warn,
    platform::collections::{HashMap, HashSet},
    ptr::OwningPtr,
    utils::synccell::SyncCell,
};

use crate::{
    app::RevSystemsPlugin,
    log::{DenseTransitionsLog, FrameTransitionLog, MissedFrame},
    meta::{NonLogNow, RevDirection, RevMeta},
};

mod bundle_buffer;
//mod commands;
mod spawn_despawn;
//mod entity_commands;
mod entity_world;
mod relationship;
mod world;

pub use bundle_buffer::*;
//pub use commands::*;
pub use spawn_despawn::*;
//pub use entity_commands::*;
pub use entity_world::*;
pub(crate) use relationship::*;
pub use world::*;

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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RevEntitiesError {
    pub invalid: Vec<EntityDoesNotExistError>,
    pub rev_despawned: Vec<EntityRevDespawnedError>,
    pub rev_despawned_buffers: MaybeLocation<Vec<EntityRevDespawnedError>>,
}

impl From<EntityRevDespawnedError> for RevEntitiesError {
    fn from(error: EntityRevDespawnedError) -> Self {
        RevEntityError::from(error).into()
    }
}

impl From<EntityDoesNotExistError> for RevEntitiesError {
    fn from(error: EntityDoesNotExistError) -> Self {
        RevEntityError::from(error).into()
    }
}

impl From<RevEntityError> for RevEntitiesError {
    fn from(error: RevEntityError) -> Self {
        let mut this = Self::empty();
        this.push(error);
        this
    }
}

impl RevEntitiesError {
    fn empty() -> Self {
        Self {
            invalid: Vec::new(),
            rev_despawned: Vec::new(),
            rev_despawned_buffers: MaybeLocation::new_with(|| Vec::new()),
        }
    }
    fn push(&mut self, error: impl Into<RevEntityError>) {
        match error.into() {
            RevEntityError::EntityDoesNotExistError(error) => self.invalid.push(error),
            RevEntityError::EntityRevDespawnedError(error) => match self
                .rev_despawned_buffers
                .as_mut()
                .zip(error.marker.added_location())
                .into_option()
            {
                Some((rev_despawned_buffers, location)) if location.is_none() => {
                    rev_despawned_buffers.push(error)
                }
                _ => self.rev_despawned.push(error),
            },
        }
    }
    fn is_empty(&self) -> bool {
        self.invalid.is_empty()
            && self.rev_despawned.is_empty()
            && self
                .rev_despawned_buffers
                .as_ref()
                .map(|rev_despawned_buffers| rev_despawned_buffers.is_empty())
                .into_option()
                .unwrap_or(true)
    }
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
                for err in self.rev_despawned.iter() {
                    let entity = err.entity;
                    let at = err.marker.added_frame();
                    write!(f, "{entity} at {at}, ")?;
                }
            }
            write!(f, "(enable `track_location` feature for more details)")?;
        } else {
            if !self.invalid.is_empty() {
                write!(f, "non-existing entities: ")?;
                for err in self.invalid.iter() {
                    write!(f, "{err}, ")?;
                }
            }
            if !self.rev_despawned.is_empty() {
                write!(f, "reversibly despawned entities: ")?;
                for err in self.rev_despawned.iter() {
                    let entity = err.entity;
                    let at = err.marker.added_frame();
                    let by = err
                        .marker
                        .added_location()
                        .into_option()
                        .flatten()
                        .expect("non-buffer entity should have a despawn location");
                    write!(f, "{entity} {by} at {at}, ")?;
                }
            }
            let rev_despawned_buffers = self.rev_despawned_buffers.as_ref().into_option().unwrap();
            if !rev_despawned_buffers.is_empty() {
                write!(f, "reversibly despawned buffer entities: ")?;
                for err in rev_despawned_buffers.iter() {
                    let entity = err.entity;
                    let at = err.marker.added_frame();
                    write!(f, "{entity} at {at}, ")?;
                }
            }
        }
        Ok(())
    }
}

impl Error for RevEntitiesError {}

// todo: split trait for _deferred variant? try to trigger bugs by using trait as-is in wrong contexts
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
    fn buffer_undo_redo(&mut self, now: NonLogNow, undo_redo: impl UndoRedo);
}

impl BuffersUndoRedo for Commands<'_, '_> {
    fn buffer_undo_redo(&mut self, now: NonLogNow, undo_redo: impl UndoRedo) {
        self.queue(move |world: &mut World| {
            world.buffer_undo_redo(now, undo_redo);
        });
    }
}

impl BuffersUndoRedo for EntityCommands<'_> {
    fn buffer_undo_redo(&mut self, now: NonLogNow, undo_redo: impl UndoRedo) {
        self.queue(move |mut world: EntityWorldMut| {
            world.buffer_undo_redo(now, undo_redo);
        });
    }
}

impl BuffersUndoRedo for World {
    fn buffer_undo_redo(&mut self, now: NonLogNow, undo_redo: impl UndoRedo) {
        DeferredWorld::buffer_undo_redo(&mut self.into(), now, undo_redo);
    }
}

impl BuffersUndoRedo for EntityWorldMut<'_> {
    fn buffer_undo_redo(&mut self, now: NonLogNow, undo_redo: impl UndoRedo) {
        // SAFETY: Only resources are accessed, entity location remains unchanged
        let world = unsafe { self.world_mut() };
        world.buffer_undo_redo(now, undo_redo);
    }
}

impl BuffersUndoRedo for DeferredWorld<'_> {
    fn buffer_undo_redo(&mut self, now: NonLogNow, undo_redo: impl UndoRedo) {
        debug_assert_eq!(
            self.get_resource::<RevMeta>().map(RevMeta::non_log_now),
            Some(Some(now))
        );
        self.get_resource_mut::<UndoRedoBuffer>()
            .expect(UndoRedoBuffer::EXPECT_IN_WORLD)
            .buffer_undo_redo(now, undo_redo);
    }
}

impl BuffersUndoRedo for UndoRedoBuffer {
    fn buffer_undo_redo(&mut self, _: NonLogNow, undo_redo: impl UndoRedo) {
        let name = type_name_of_val(&undo_redo);
        let boxed = BoxedUndoRedo {
            undo_redo: SyncCell::new(Box::new(undo_redo)),
            name,
        };
        self.0.push(boxed);
    }
}

/// For usages in _observer_ systems. Regular reversible systems should use commands or &mut World.
///
/// Commands and hooks can buffer [`UndoRedo`] implementors via [`&mut World`](World)/[`DeferredWorld`] instead.
///
/// Do not remove or overwrite this resource.
#[derive(Resource, Default, Debug)] // todo: wrap in private resource
pub struct UndoRedoBuffer(Vec<BoxedUndoRedo>);

impl UndoRedoBuffer {
    pub(crate) const EXPECT_IN_WORLD: &'static str =
        "BuffersUndoRedo methods need the UndoRedoBuffer resource but it is missing";
    /// Returns `true` when the buffer is empty, otherwise returns `false`.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
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

pub struct DeferredUndoRedo<T: UndoRedo, F: FnOnce(&mut World) -> T + Send + 'static>(
    MaybeUninitUndoRedo<T, F>,
);

enum MaybeUninitUndoRedo<T: UndoRedo, F: FnOnce(&mut World) -> T + Send + 'static> {
    Uninit(Option<F>),
    Init(T),
}

impl<T: UndoRedo, F: FnOnce(&mut World) -> T + Send + 'static> DeferredUndoRedo<T, F> {
    pub fn new(undo: F) -> Self {
        Self(MaybeUninitUndoRedo::Uninit(Some(undo)))
    }
}

// todo: change fn generic type here to FromWorld function item when https://github.com/rust-lang/rust/issues/63063 is stabilized
impl<T: UndoRedo + FromWorld> Default for DeferredUndoRedo<T, fn(&mut World) -> T> {
    fn default() -> Self {
        Self::new(T::from_world)
    }
}

impl<T: UndoRedo, F: FnOnce(&mut World) -> T + Send + 'static> UndoRedo for DeferredUndoRedo<T, F> {
    fn undo(&mut self, world: &mut World) {
        match &mut self.0 {
            MaybeUninitUndoRedo::Uninit(undo) => {
                let undo = undo.take().unwrap();
                self.0 = MaybeUninitUndoRedo::Init(undo(world))
            }
            MaybeUninitUndoRedo::Init(init) => init.undo(world),
        }
    }
    fn redo(&mut self, world: &mut World) {
        match &mut self.0 {
            MaybeUninitUndoRedo::Init(init) => init.redo(world),
            MaybeUninitUndoRedo::Uninit(_) => panic!("todo"),
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

#[derive(Copy, Clone, Debug)]
pub struct UndoRedoSwap<T: UndoRedo>(pub T);

impl<T: UndoRedo> UndoRedo for UndoRedoSwap<T> {
    fn undo(&mut self, world: &mut World) {
        self.0.redo(world);
    }
    fn redo(&mut self, world: &mut World) {
        self.0.undo(world);
    }
}

impl<T: UndoRedo + FromWorld> FromWorld for UndoRedoSwap<T> {
    fn from_world(world: &mut World) -> Self {
        Self(T::from_world(world))
    }
}

#[expect(dead_code)] // https://github.com/bevyengine/bevy/pull/18880
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

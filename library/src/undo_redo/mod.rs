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
        bundle::{Bundle, BundleId, InsertMode},
        change_detection::Mut,
        component::{Component, ComponentCloneBehavior, ComponentId, ComponentMutability},
        entity::{Entity, EntityCloner, hash_set::EntityHashSet},
        hierarchy::{ChildOf, Children},
        query::{Has, QueryData, QueryFilter, ReadOnlyQueryData, With, WorldQuery},
        relationship::{OrderedRelationshipSourceCollection, Relationship, RelationshipTarget},
        resource::Resource,
        system::{Commands, EntityCommands},
        world::{
            DeferredWorld, EntityMut, EntityMutExcept, EntityRef, EntityRefExcept, EntityWorldMut,
            FilteredEntityMut, FilteredEntityRef, World,
        },
    },
    platform::collections::{HashMap, HashSet},
    utils::synccell::SyncCell,
};

use crate::{
    log::{DenseTransitionsLog, FrameTransitionLog, FrameTransitionLogError},
    meta::{RevDirection, RevMeta},
};

mod bundle_buffer;
mod commands;
mod despawn;
mod entity_commands;
mod entity_world;
mod world;

pub use bundle_buffer::*;
pub use commands::*;
pub use despawn::*;
pub use entity_commands::*;
pub use entity_world::*;
pub use world::*;

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

    /// Pops the most recently pushed `UndoRedo` by a [`BuffersUndoRedo`] implementor which may be deferred.
    ///
    /// This will make it unobtainable for the internal log of the reversible system.
    ///
    /// Because of this, **this method should only be used for tests of `UndoRedo` implementors**.
    ///
    /// The type equality is asserted on the names returned by [`type_name`] which may be not fully accurate.
    ///
    /// In other cases consider to use [Self::type_names] or the `Debug` implemention to inspect which types are contained.
    ///
    /// Assertions on the buffer being empty can be done with [Self::is_empty] instead.
    ///
    /// # Panics
    ///
    /// This method panics if the inner collection is empty or if the popped type has a different name than `T`.
    pub fn pop_assert_type<T: UndoRedo>(&mut self) -> Box<dyn UndoRedo> {
        let expected = type_name::<T>();
        let boxed = self
            .0
            .pop()
            .unwrap_or_else(|| panic!("expected `Some({expected})`, found `None`"));
        assert_eq!(boxed.name, expected);
        SyncCell::to_inner(boxed.undo_redo)
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
) -> impl FnOnce(FrameTransitionLogError) -> UndoRedoLogError + '_ {
    move |err| match err {
        FrameTransitionLogError::MissedFrame(frame) => UndoRedoLogError::MissedFrame {
            frame,
            now,
            system_name: system_name.to_owned(),
        },
        FrameTransitionLogError::OutOfLog => UndoRedoLogError::OutOfLog {
            now,
            system_name: system_name.to_owned(),
        },
    }
}

fn collect_children(
    world: &World,
    entity: Entity,
    component_id: ComponentId,
    entities: &mut EntityHashSet,
) {
    let entity_mut = world.entity(entity);
    if entity_mut.contains_id(component_id) {
        return;
    }
    if !entities.insert(entity) {
        return;
    }
    let Some(children) = entity_mut.get::<Children>() else {
        return;
    };
    for &child in children {
        collect_children(world, child, component_id, entities);
    }
}

fn rev_despawn_inner(mut entity_mut: EntityWorldMut) -> bool {
    let component_id = entity_mut
        .world()
        .component_id::<DespawnAtOutOfLog>()
        .unwrap();
    if entity_mut.contains_id(component_id) {
        return false;
    }

    let meta = entity_mut
        .get_resource::<RevMeta>()
        .expect(RevMeta::EXPECT_IN_WORLD);
    let marker = DespawnAtOutOfLog::new(meta);

    let entity = entity_mut.id();
    let children = entity_mut
        .get::<Children>()
        .map(|children| RelationshipTarget::iter(children).collect::<Vec<Entity>>())
        .filter(|children| !children.is_empty());

    entity_mut.world_scope(|world| {
        let Some(children) = children else {
            return rev_despawn_single(world, entity, marker);
        };
        let mut entities = [entity].into_iter().collect();
        for child in children {
            collect_children(world, child, component_id, &mut entities);
        }
        if entities.len() < 2 {
            return rev_despawn_single(world, entity, marker);
        }

        let mut undo_redo = RevDespawnHierarchy {
            entities: entities.into_iter().collect(),
            marker,
        };
        undo_redo.redo(world);
        world.buffer_undo_redo(undo_redo);

        true
    })
}

fn rev_despawn_single(world: &mut World, entity: Entity, marker: DespawnAtOutOfLog) -> bool {
    let mut undo_redo = RevDespawnSingle { entity, marker };
    undo_redo.redo(world);
    world.buffer_undo_redo(undo_redo);
    true
}

struct RevDespawnSingle {
    entity: Entity,
    marker: DespawnAtOutOfLog,
}

impl UndoRedo for RevDespawnSingle {
    fn undo(&mut self, world: &mut World) {
        world.entity_mut(self.entity).remove::<DespawnAtOutOfLog>();
    }
    fn redo(&mut self, world: &mut World) {
        world.entity_mut(self.entity).insert(self.marker);
    }
}

struct RevDespawnHierarchy {
    entities: Arc<[Entity]>,
    marker: DespawnAtOutOfLog,
}

impl UndoRedo for RevDespawnHierarchy {
    fn undo(&mut self, world: &mut World) {
        let component_id = world.component_id::<DespawnAtOutOfLog>().expect("todo");
        let mut commands = world.commands();
        for &entity in self.entities.iter().rev() {
            commands.entity(entity).remove_by_id(component_id);
        }
        world.flush();
    }
    fn redo(&mut self, world: &mut World) {
        struct Iter {
            entities: Arc<[Entity]>,
            index: usize,
            marker: DespawnAtOutOfLog,
        }

        impl Iterator for Iter {
            type Item = (Entity, DespawnAtOutOfLog);
            fn next(&mut self) -> Option<Self::Item> {
                self.entities.get(self.index).map(|entity| {
                    self.index += 1;
                    (*entity, self.marker)
                })
            }
            fn size_hint(&self) -> (usize, Option<usize>) {
                let len = self.len();
                (len, Some(len))
            }
        }

        impl ExactSizeIterator for Iter {
            fn len(&self) -> usize {
                self.entities.len() - self.index
            }
        }

        impl FusedIterator for Iter {}

        world.insert_batch(Iter {
            entities: self.entities.clone(),
            index: 0,
            marker: self.marker,
        })
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

struct Spawn {
    entity: Entity,
    marker: DespawnAtOutOfLog,
}

impl UndoRedo for Spawn {
    fn undo(&mut self, world: &mut World) {
        world.entity_mut(self.entity).insert(self.marker);
    }
    fn redo(&mut self, world: &mut World) {
        world.entity_mut(self.entity).remove::<DespawnAtOutOfLog>();
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

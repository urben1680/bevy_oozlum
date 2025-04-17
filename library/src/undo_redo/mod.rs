use std::{
    any::{type_name, type_name_of_val}, error::Error, fmt::{Debug, Display}, hash::Hash, iter::FusedIterator, marker::PhantomData, sync::Arc
};

use bevy::{
    ecs::{
        archetype::ArchetypeId, bundle::{Bundle, BundleId, InsertMode}, change_detection::Mut, component::{Component, ComponentCloneBehavior, ComponentId, ComponentMutability}, entity::{hash_set::EntityHashSet, Entity, EntityCloner}, hierarchy::Children, query::{Has, QueryData, QueryFilter, ReadOnlyQueryData, With, WorldQuery}, resource::Resource, system::{Commands, EntityCommands}, world::{DeferredWorld, EntityMut, EntityMutExcept, EntityRef, EntityRefExcept, EntityWorldMut, FilteredEntityMut, FilteredEntityRef, World}
    },
    platform::collections::{HashMap, HashSet},
    utils::synccell::SyncCell,
};

use crate::{
    log::{DenseTransitionsLog, FrameTransitionLog, FrameTransitionLogError},
    meta::{RevDirection, RevMeta},
};

mod commands;
mod entity;
mod world;

pub use commands::*;
pub use entity::*;
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

pub struct UndoRedoSwap<T: UndoRedo>(pub T);

impl<F: FnMut(&mut World, UndoRedoDirection) + Send + 'static> UndoRedo for F {
    fn undo(&mut self, world: &mut World) {
        self(world, UndoRedoDirection::Undo)
    }
    fn redo(&mut self, world: &mut World) {
        self(world, UndoRedoDirection::Redo)
    }
}

impl<T: UndoRedo> UndoRedo for UndoRedoSwap<T> {
    fn undo(&mut self, world: &mut World) {
        self.0.redo(world);
    }
    fn redo(&mut self, world: &mut World) {
        self.0.undo(world);
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

#[derive(Resource)]
pub(crate) struct BufferComponentsInProgress;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum BufferAt {
    /// The components will be buffered now which removes them from the entity.
    ///
    /// When this is undone, the components are moved back into the entity from the buffer.
    ///
    /// Redoing this results in buffering and removing them from the entity again.
    ///
    /// This variant is useful as reversible removals of these components.
    ///
    /// **Make sure to not manually remove the components and solely use this buffering.**
    Now,
    /// The components will be buffered when this action is undone, which then removed them
    /// from the entity. Until then they remain at the entity.
    ///
    /// When this id redone, the components are moved back into the entity from the buffer.
    ///
    /// This variant is useful to make accompanied insertions of these components _without_
    /// overwrites reversible.
    Undo,
    /// Combines [`BufferAt::Now`] and [`BufferAt::Undo`], utilizing two separate buffers.
    ///
    /// This variant is useful to make accompanied insertions of these components _with_
    /// overwrites reversible.
    ///
    /// **Make sure to do such insertions right after and not before this buffering.**
    NowAndUndo,
}

#[derive(Component, Clone, Copy, Debug, Hash, PartialOrd, Ord, PartialEq, Eq)]
#[component(immutable)]
pub(crate) struct DespawnAtOutOfLog(u64);

impl DespawnAtOutOfLog {
    pub(crate) fn new(meta: &RevMeta) -> Self {
        assert_eq!(meta.get_running_direction(), Some(RevDirection::NOT_LOG));
        Self(meta.now())
    }
}

#[derive(QueryFilter)]
pub struct WithRevDespawned {
    _filter: With<DespawnAtOutOfLog>
}

pub struct HasRevDespawned;

// SAFETY: same as Has
unsafe impl WorldQuery for HasRevDespawned {
    type Fetch<'a> = bool;
    type State = ComponentId;

    const IS_DENSE: bool = <Has<DespawnAtOutOfLog> as WorldQuery>::IS_DENSE;

    fn shrink_fetch<'wlong: 'wshort, 'wshort>(fetch: Self::Fetch<'wlong>) -> Self::Fetch<'wshort> {
        <Has<DespawnAtOutOfLog> as WorldQuery>::shrink_fetch(fetch)
    }

    unsafe fn init_fetch<'w>(
            world: bevy::ecs::world::unsafe_world_cell::UnsafeWorldCell<'w>,
            state: &Self::State,
            last_run: bevy::ecs::component::Tick,
            this_run: bevy::ecs::component::Tick,
        ) -> Self::Fetch<'w> {
            unsafe {
                // SAFETY: same as Has
                <Has<DespawnAtOutOfLog> as WorldQuery>::init_fetch(world, state, last_run, this_run)
            }
    }

    unsafe fn set_archetype<'w>(
            fetch: &mut Self::Fetch<'w>,
            state: &Self::State,
            archetype: &'w bevy::ecs::archetype::Archetype,
            table: &'w bevy::ecs::storage::Table,
        ) {
            unsafe {
                // SAFETY: same as Has
            <Has<DespawnAtOutOfLog> as WorldQuery>::set_archetype(fetch, state, archetype, table)
        }
    }

    unsafe fn set_table<'w>(fetch: &mut Self::Fetch<'w>, state: &Self::State, table: &'w bevy::ecs::storage::Table) {
        unsafe {
            // SAFETY: same as Has
        <Has<DespawnAtOutOfLog> as WorldQuery>::set_table(fetch, state, table)
    }
    }

    fn update_component_access(state: &Self::State, access: &mut bevy::ecs::query::FilteredAccess<ComponentId>) {
        <Has<DespawnAtOutOfLog> as WorldQuery>::update_component_access(state, access)
    }

    fn init_state(world: &mut World) -> Self::State {
        <Has<DespawnAtOutOfLog> as WorldQuery>::init_state(world)
    }

    fn get_state(components: &bevy::ecs::component::Components) -> Option<Self::State> {
        <Has<DespawnAtOutOfLog> as WorldQuery>::get_state(components)
    }

    fn matches_component_set(
            state: &Self::State,
            set_contains_id: &impl Fn(ComponentId) -> bool,
        ) -> bool {
            <Has<DespawnAtOutOfLog> as WorldQuery>::matches_component_set(state, set_contains_id)
    }

    fn set_access(state: &mut Self::State, access: &bevy::ecs::query::FilteredAccess<ComponentId>) {
        <Has<DespawnAtOutOfLog> as WorldQuery>::set_access(state, access);
    }
}

// SAFETY: same as Has
unsafe impl QueryData for HasRevDespawned {
    type ReadOnly = Self;
    type Item<'a> = bool;

    const IS_READ_ONLY: bool = true;

    fn shrink<'wlong: 'wshort, 'wshort>(item: Self::Item<'wlong>) -> Self::Item<'wshort> {
        <Has<DespawnAtOutOfLog> as QueryData>::shrink(item)
    }

    unsafe fn fetch<'w>(
            fetch: &mut Self::Fetch<'w>,
            entity: Entity,
            table_row: bevy::ecs::storage::TableRow,
        ) -> Self::Item<'w> {
            unsafe {
                // SAFETY: same as Has
            <Has<DespawnAtOutOfLog> as QueryData>::fetch(fetch, entity, table_row)
            }
    }
}

// SAFETY: same as Has
unsafe impl ReadOnlyQueryData for HasRevDespawned {}

#[derive(QueryData)]
#[doc(hidden)]
pub struct RefRevDespawned {
    marker: &'static DespawnAtOutOfLog
}

impl RefRevDespawnedItem<'_> {
    pub(crate) fn added_at(&self) -> u64 {
        self.marker.0
    }
}

pub trait RevIsDespawned {
    fn rev_is_despawned(&self) -> bool;
}

impl RevIsDespawned for EntityRef<'_> {
    fn rev_is_despawned(&self) -> bool {
        self.contains::<DespawnAtOutOfLog>()
    }
}

impl<B: Bundle> RevIsDespawned for EntityRefExcept<'_, B> {
    fn rev_is_despawned(&self) -> bool {
        self.contains::<DespawnAtOutOfLog>()
    }
}

impl RevIsDespawned for FilteredEntityRef<'_> {
    fn rev_is_despawned(&self) -> bool {
        self.contains::<DespawnAtOutOfLog>()
    }
}

impl RevIsDespawned for EntityMut<'_> {
    fn rev_is_despawned(&self) -> bool {
        self.contains::<DespawnAtOutOfLog>()
    }
}

impl<B: Bundle> RevIsDespawned for EntityMutExcept<'_, B> {
    fn rev_is_despawned(&self) -> bool {
        self.contains::<DespawnAtOutOfLog>()
    }
}

impl RevIsDespawned for FilteredEntityMut<'_> {
    fn rev_is_despawned(&self) -> bool {
        self.contains::<DespawnAtOutOfLog>()
    }
}

impl RevIsDespawned for EntityWorldMut<'_> {
    fn rev_is_despawned(&self) -> bool {
        self.contains::<DespawnAtOutOfLog>()
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

struct RevDespawnSingle {
    entity: Entity,
    marker: DespawnAtOutOfLog,
}

struct RevDespawnHierarchy {
    entities: Arc<[Entity]>,
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

fn pre_insert<T: Bundle>(
    world: &mut World,
    entity: Entity,
    archetype_id: ArchetypeId,
    insert_mode: InsertMode,
) {
    match insert_mode {
        InsertMode::Replace => world.buffer_components_cached(
            entity,
            unique_for_location!(archetype_id, PhantomData::<T>),
            |world: &mut World| {
                let bundle_id = world.register_bundle::<T>().id();
                insert_maybe_overwrite(world, bundle_id, archetype_id)
            },
        ),
        InsertMode::Keep => world.buffer_components_cached(
            entity,
            unique_for_location!(archetype_id, PhantomData::<T>),
            |world| {
                let bundle_id = world.register_bundle::<T>().id();
                insert_no_overwrite(&world, bundle_id, archetype_id)
            },
        ),
    };
}

fn rev_despawn_single(world: &mut World, entity: Entity, marker: DespawnAtOutOfLog) -> bool {
    let mut undo_redo = RevDespawnSingle { entity, marker };
    undo_redo.redo(world);
    world.buffer_undo_redo(undo_redo);
    true
}

fn rev_despawn_children(
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
        rev_despawn_children(world, child, component_id, entities);
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

    let at = entity_mut.get_resource::<RevMeta>().expect(RevMeta::EXPECT_IN_WORLD).now();
    let marker = DespawnAtOutOfLog(at);

    let entity = entity_mut.id();
    let children = entity_mut
        .get::<Children>()
        .map(|children| children.iter().copied().collect::<Vec<Entity>>())
        .filter(|children| !children.is_empty());

    entity_mut.world_scope(|world| {
        let Some(children) = children else {
            return rev_despawn_single(world, entity, marker);
        };
        let mut entities = [entity].into_iter().collect();
        for child in children {
            rev_despawn_children(world, child, component_id, &mut entities);
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

fn insert_maybe_overwrite(
    world: &World,
    bundle_id: BundleId,
    archetype_id: ArchetypeId,
) -> (BufferAt, Vec<ComponentId>) {
    // Bundle explicit:  A(2), B(2), C(2)
    // Bundle required:                    D(2), E(2)

    // Entity before:    A(1), B(1),             E(1)
    // Entity after:     A(2), B(2), C(2), D(2), E(1)

    // Buffer 1:         A(1), B(1), C(*), D(*)        *if any appear at redo
    // Buffer 2 at undo: A(2), B(2), C(2), D(2)

    let bundle = world.bundles().get(bundle_id).unwrap();
    let archetype = world.archetypes().get(archetype_id).unwrap();
    let components = bundle
        .explicit_components()
        .iter()
        .chain(
            bundle
                .required_components()
                .iter()
                .filter(|component_id| !archetype.contains(**component_id)),
        )
        .copied()
        .collect();
    let overwrites = bundle
        .explicit_components()
        .iter()
        .any(|component_id| archetype.contains(*component_id));
    let at = if overwrites {
        BufferAt::NowAndUndo
    } else {
        BufferAt::Undo
    };
    (at, components)
}

fn insert_no_overwrite(
    world: &World,
    bundle_id: BundleId,
    archetype_id: ArchetypeId,
) -> (BufferAt, Vec<ComponentId>) {
    // Bundle explicit:  A(2), B(2), C(2)
    // Bundle required:                    D(2), E(2)

    // Entity before:    A(1), B(1),             E(1)
    // Entity after:     A(1), B(1), C(2), D(2), E(1)

    // Buffer at undo:               C(2), D(2)

    let archetype = world.archetypes().get(archetype_id).unwrap();
    let components = world
        .bundles()
        .get(bundle_id)
        .unwrap()
        .contributed_components()
        .iter()
        .copied()
        .filter(|component_id| !archetype.contains(*component_id))
        .collect();
    (BufferAt::Undo, components)
}

#[derive(Resource, Default)]
struct NonEntityBufferRes(HashMap<ComponentId, fn(&mut World, Entity, BufferAt)>);

fn non_entity_buffer(world: &mut World, entity: Entity, at: BufferAt, components: &[ComponentId]) {
    if !world.contains_resource::<NonEntityBufferRes>() {
        return;
    }
    world.resource_scope(|world, non_entity_buffers: Mut<NonEntityBufferRes>| {
        for component in components.iter() {
            if let Some(c) = non_entity_buffers.0.get(component) {
                c(world, entity, at);
            }
        }
    })
}

pub(crate) fn register_non_entity_buffer<T: Component>(world: &mut World) {
    struct NonEntityBuffer<T: Component> {
        entity: Entity,
        component: Option<T>,
    }

    impl<T: Component> UndoRedo for NonEntityBuffer<T> {
        fn undo(&mut self, world: &mut World) {
            let mut entity = world.entity_mut(self.entity);
            if T::Mutability::MUTABLE {
                let component = unsafe {
                    // SAFETY: this if branch asserts the component is mutable
                    entity.get_mut_assume_mutable::<T>()
                };
                match component {
                    Some(mut c1) => match self.component.as_mut() {
                        Some(c2) => core::mem::swap(&mut *c1, c2),
                        None => self.component = entity.take::<T>(),
                    },
                    None => {
                        if let Some(c2) = self.component.take() {
                            entity.insert(c2);
                        }
                    }
                }
            } else {
                match entity.take::<T>() {
                    Some(mut c1) => match self.component.as_mut() {
                        Some(c2) => {
                            core::mem::swap(&mut c1, c2);
                            entity.insert(c1);
                        }
                        None => self.component = Some(c1),
                    },
                    None => {
                        if let Some(c2) = self.component.take() {
                            entity.insert(c2);
                        }
                    }
                }
            }
        }
        fn redo(&mut self, world: &mut World) {
            self.undo(world);
        }
    }

    let component_id = world.register_component::<T>();
    world.get_resource_or_init::<NonEntityBufferRes>().0.insert(
        component_id,
        |world, entity, at| {
            let mut component = None;
            if matches!(at, BufferAt::Now | BufferAt::NowAndUndo) {
                component = world.entity_mut(entity).take::<T>();
            }
            let undo_redo = NonEntityBuffer { entity, component };
            world.buffer_undo_redo(undo_redo);
        },
    );
}

fn buffer_bundle(
    world: &mut World,
    entity: Entity,
    at: BufferAt,
    bundle: BundleId,
) -> Option<EntityRef> {
    let mut buffer = BundleBuffer::new(world, entity, bundle);
    match at {
        BufferAt::Now => {
            let entities = buffer.toggle_state(world);
            let components = buffer.get_component_ids(world);
            non_entity_buffer(world, entity, at, &components);
            let out = buffer.move_bundle(world, entities, &components);
            world.buffer_undo_redo(buffer);
            Some(world.entity(out))
        }
        BufferAt::Undo => {
            let components = buffer.get_component_ids(world);
            non_entity_buffer(world, entity, at, &components);
            world.buffer_undo_redo(buffer);
            None
        }
        BufferAt::NowAndUndo => {
            let at_undo = buffer.clone(); // no buffer entity set yet so each spawns their own
            let entities = buffer.toggle_state(world);
            let components = buffer.get_component_ids(world);
            non_entity_buffer(world, entity, at, &components);
            let out = buffer.move_bundle(world, entities, &components);
            world.buffer_undo_redo(buffer).buffer_undo_redo(at_undo);
            Some(world.entity(out))
        }
    }
}

#[derive(Clone)]
struct BundleBuffer {
    bundle: BundleId,
    entity: Entity,
    state: BufferState,
}

#[derive(Clone)]
enum BufferState {
    Unspawned(DespawnAtOutOfLog),
    Empty(Entity),
    Filled(Entity),
}

struct BundleEntities {
    target: Entity,
    source: Entity,
    buffer: Entity,
}

impl BundleBuffer {
    fn new(world: &World, entity: Entity, bundle: BundleId) -> Self {
        let meta = world.get_resource::<RevMeta>().expect(RevMeta::EXPECT_IN_WORLD);
        let marker = DespawnAtOutOfLog::new(meta);
        Self {
            bundle,
            entity,
            state: BufferState::Unspawned(marker),
        }
    }
    fn toggle_state(&mut self, world: &mut World) -> BundleEntities {
        match self.state {
            BufferState::Unspawned(marker) => {
                let buffer = world.spawn(marker).id();
                self.state = BufferState::Filled(buffer);
                BundleEntities {
                    target: buffer,
                    source: self.entity,
                    buffer,
                }
            }
            BufferState::Filled(buffer) => {
                self.state = BufferState::Empty(buffer);
                BundleEntities {
                    target: self.entity,
                    source: buffer,
                    buffer,
                }
            }
            BufferState::Empty(buffer) => {
                self.state = BufferState::Filled(buffer);
                BundleEntities {
                    target: buffer,
                    source: self.entity,
                    buffer,
                }
            }
        }
    }
    fn get_component_ids(&self, world: &World) -> Box<[ComponentId]> {
        world
            .bundles()
            .get(self.bundle)
            .expect("todo")
            .explicit_components()
            .into()
    }
    fn move_bundle(
        &mut self,
        world: &mut World,
        entities: BundleEntities,
        components: &[ComponentId],
    ) -> Entity {
        let progress_res = world.buffer_components_in_progress();
        if !progress_res {
            world.insert_resource(BufferComponentsInProgress);
        }
        EntityCloner::build(world)
            .deny_all()
            .move_components(true)
            .without_required_components(|builder| {
                builder.allow_by_ids(components.iter().copied());
            })
            .clone_entity(entities.source, entities.target);
        if !progress_res {
            world.remove_resource::<BufferComponentsInProgress>();
        }
        entities.buffer
    }
}

impl UndoRedo for BundleBuffer {
    fn undo(&mut self, world: &mut World) {
        let entities = self.toggle_state(world);
        let components = self.get_component_ids(world);
        self.move_bundle(world, entities, &components);
    }
    fn redo(&mut self, world: &mut World) {
        self.undo(world);
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

// todo: replace this with register_dynamic_bundle when linked issue is fixed
fn components_to_bundle(world: &mut World, components: Vec<ComponentId>) -> BundleId {
    #[derive(Resource, Default)]
    struct CheckedClonable(HashSet<ComponentId>);

    let mut checked = world
        .remove_resource::<CheckedClonable>()
        .unwrap_or_default();
    for &component_id in &components {
        if checked.0.insert(component_id) {
            if let Some(component_info) = world.components().get_info(component_id) {
                if component_info.clone_behavior() == &ComponentCloneBehavior::Ignore {
                    bevy::log::error!(
                        "Component {} is unclonable, it's insert, remove or overwrite will \
                        not be reversible, see https://github.com/bevyengine/bevy/issues/18079",
                        component_info.name()
                    );
                }
            }
        }
    }
    world.insert_resource(checked);

    world.register_dynamic_bundle(&components).id()
}

#[macro_export]
macro_rules! unique_for_location {
    ($($hashable: expr),*) => {
        // extra scope to keep `UniquePerInvoke`s isolated
        {
            struct UniquePerInvoke;
            (std::any::TypeId::of::<UniquePerInvoke>(), $($hashable,)*)
        }
    }
}

use unique_for_location;

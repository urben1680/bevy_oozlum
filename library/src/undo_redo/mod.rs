use std::{
    collections::VecDeque,
    error::Error,
    fmt::{Debug, Display},
    hash::{BuildHasher, Hash, Hasher},
    iter::FusedIterator,
    sync::Arc,
};

use bevy::{
    ecs::{
        archetype::{Archetype, ArchetypeGeneration, ArchetypeId, Archetypes}, component::{Component, ComponentCloneBehavior, ComponentId}, entity::{hash_set::EntityHashSet, Entity, EntityCloner}, hierarchy::Children, query::{QueryBuilder, QueryState, With}, resource::Resource, storage::TableId, system::{Commands, EntityCommands}, world::{DeferredWorld, EntityWorldMut, FromWorld, World}
    },
    log::error,
    platform_support::{
        collections::{hash_map::Entry, HashMap, HashSet},
        hash::{FixedHasher, PassHash},
    },
    utils::synccell::SyncCell,
};

use crate::{
    log::{DenseTransitionsLog, FrameTransitionLog},
    meta::{RevDirection, RevMeta},
};

mod commands;
mod entity_commands;

pub use commands::*;
pub use entity_commands::*;

// todo rename
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
    /// | [`EntityWorldMut`] | ✅ | ❌ |
    /// | [`DeferredWorld`] | ✅ | ❌ |
    /// | [`RevBuffers`] | ✅ | ❌ |
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
        self.get_resource_mut::<RevBuffers>()
            .expect(EXPECT_BUFFER)
            .buffer_undo_redo(undo_redo);
        self
    }
}

impl BuffersUndoRedo for DeferredWorld<'_> {
    fn buffer_undo_redo(&mut self, undo_redo: impl UndoRedo) -> &mut Self {
        self.get_resource_mut::<RevBuffers>()
            .expect(EXPECT_BUFFER)
            .buffer_undo_redo(undo_redo);
        self
    }
}

impl BuffersUndoRedo for RevBuffers {
    fn buffer_undo_redo(&mut self, undo_redo: impl UndoRedo) -> &mut Self {
        self.undo_redo_buffer
            .push_back(SyncCell::new(Box::new(undo_redo)));
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
}

impl RevBuffers {
    pub fn undo_redo_is_empty(&self) -> bool {
        self.undo_redo_buffer.is_empty()
    }
    #[cfg(test)]
    pub fn pop_undo_redo(&mut self) -> Option<Box<dyn UndoRedo>> {
        self.undo_redo_buffer.pop_back().map(SyncCell::to_inner)
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

pub trait RevWorld {
    fn rev_despawn(&mut self, entity: Entity) -> bool;
    fn rev_buffer_components(
        &mut self,
        entity: Entity,
        components: impl IntoIterator<Item = ComponentId>,
    ) -> bool;
    fn rev_buffer_components_cached<I: IntoIterator<Item = ComponentId>>(
        &mut self,
        entity: Entity,
        cache: impl Hash,
        components: impl FnOnce(&mut World) -> I,
    ) -> bool;
    fn rev_buffer_components_at_undo(
        &mut self,
        entity: Entity,
        components: impl IntoIterator<Item = ComponentId>,
    ) -> bool;
    fn rev_buffer_components_at_undo_cached<I: IntoIterator<Item = ComponentId>>(
        &mut self,
        entity: Entity,
        cache: impl Hash,
        components: impl FnOnce(&mut World) -> I,
    ) -> bool;
    fn ongoing_component_buffer(&self) -> bool;
}

impl RevWorld for World {
    fn rev_despawn(&mut self, entity: Entity) -> bool {
        let entity_mut = self.entity_mut(entity);
        rev_despawn_inner(entity_mut)
    }
    fn rev_buffer_components(
        &mut self,
        entity: Entity,
        components: impl IntoIterator<Item = ComponentId>,
    ) -> bool {
        self.resource_scope::<ComponentBufferRes, _>(|world, mut component_buffers| {
            component_buffers
                .data
                .get(world, entity, components)
                .map(|mut buffer| {
                    buffer.move_components(world);
                    world.buffer_undo_redo(buffer);
                })
                .is_some()
        })
    }
    fn rev_buffer_components_cached<I: IntoIterator<Item = ComponentId>>(
        &mut self,
        entity: Entity,
        cache: impl Hash,
        components: impl FnOnce(&mut World) -> I,
    ) -> bool {
        self.resource_scope::<ComponentBufferRes, _>(|world, mut component_buffers| {
            component_buffers
                .get_cached(world, entity, cache, components)
                .map(|mut buffer| {
                    buffer.move_components(world);
                    world.buffer_undo_redo(buffer);
                })
                .is_some()
        })
    }
    fn rev_buffer_components_at_undo(
        &mut self,
        entity: Entity,
        components: impl IntoIterator<Item = ComponentId>,
    ) -> bool {
        self.resource_scope::<ComponentBufferRes, _>(|world, mut component_buffers| {
            component_buffers
                .data
                .get(world, entity, components)
                .map(|buffer| world.buffer_undo_redo(buffer))
                .is_some()
        })
    }
    fn rev_buffer_components_at_undo_cached<I: IntoIterator<Item = ComponentId>>(
        &mut self,
        entity: Entity,
        cache: impl Hash,
        components: impl FnOnce(&mut World) -> I,
    ) -> bool {
        self.resource_scope::<ComponentBufferRes, _>(|world, mut component_buffers| {
            component_buffers
                .get_cached(world, entity, cache, components)
                .map(|buffer| world.buffer_undo_redo(buffer))
                .is_some()
        })
    }
    fn ongoing_component_buffer(&self) -> bool {
        self.get_resource::<ComponentBufferRes>()
            .is_some_and(|component_buffers| component_buffers.ongoing_buffer)
    }
}

#[derive(Component, Clone, Copy, Debug, Hash, PartialOrd, Ord, PartialEq, Eq)]
#[component(immutable)]
pub struct DespawnAtOutOfLog(u64);

impl DespawnAtOutOfLog {
    pub fn new(now: u64) -> Self {
        Self(now)
    }
    pub fn added_at(self) -> u64 {
        self.0
    }
}

impl FromWorld for DespawnAtOutOfLog {
    fn from_world(world: &mut World) -> Self {
        (&*world).into()
    }
}

impl<'a> From<&'a World> for DespawnAtOutOfLog {
    fn from(world: &'a World) -> Self {
        world.get_resource::<RevMeta>().expect("todo").into()
    }
}

impl<'a> From<&'a RevMeta> for DespawnAtOutOfLog {
    fn from(meta: &'a RevMeta) -> Self {
        Self::new(meta.now())
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

#[derive(Component, Clone, Copy)]
#[component(immutable)]
pub struct SharedBuffer;

struct ComponentBufferDataDraft {
    components: Box<[ComponentId]>,
    unused_required: Box<[ComponentId]>,
    archetypes_without_components: Vec<ArchetypeId>,
    archetypes_moved_from: usize,
    generation: ArchetypeGeneration
}

impl ComponentBufferDataDraft {
    fn update(&mut self, world: &World) {
        let archetypes = world.archetypes();
        for new_archetype in &archetypes[self.generation..] {

        }
        self.generation = archetypes.generation();
    }
    fn get_buffer(&mut self, ) -> Entity {
        todo!()
    }
}

struct ComponentBufferData {
    query_state: QueryState<(Entity, &'static DespawnAtOutOfLog), With<SharedBuffer>>,
    components: Box<[ComponentId]>,
    // unused_required_components
}

impl ComponentBufferData {
    fn get_buffer(&mut self, world: &mut World) -> Entity {
        let meta = world.get_resource::<RevMeta>().expect("todo");
        assert_eq!(meta.get_direction(), Some(RevDirection::NOT_LOG));
        let now = meta.now();
        let bundle = (DespawnAtOutOfLog::new(now), SharedBuffer);
        self.query_state
            .iter(&world)
            .filter(|(_, marker)| marker.added_at() == now)
            .map(|(entity, _)| entity)
            .next()
            .unwrap_or_else(|| world.spawn(bundle).id())
    }
}

#[derive(Default)]
struct ComponentBufferMap {
    map: HashMap<u64, ComponentBufferData, PassHash>,
    unclonable: HashSet<ComponentId>,
}

impl ComponentBufferMap {
    fn get(
        &mut self,
        world: &mut World,
        entity: Entity,
        components: impl IntoIterator<Item = ComponentId>,
    ) -> Option<ComponentBuffer> {
        let mut components: Box<[ComponentId]> = components
            .into_iter()
            .filter(|component_id| {
                // todo: remove this filter and early return when linked issue is fixed
                let component_info = world.components().get_info(*component_id).unwrap();
                if component_info.clone_behavior() != &ComponentCloneBehavior::Ignore {
                    return true
                }
                if self.unclonable.insert(*component_id) {
                    error!(
                        "Unclonable component {} will be ignored by ComponentBuffer, it's insert, remove or \
                        overwrite will not be reversible, see https://github.com/bevyengine/bevy/issues/18079",
                        component_info.name()
                    );
                }
                false
            })
            .collect();
        if components.is_empty() {
            return None;
        }
        components.sort_unstable();
        let mut hasher = FixedHasher::default().build_hasher();
        for component_id in components.iter().copied() {
            component_id.hash(&mut hasher);
        }
        let key = hasher.finish();
        let data = self.map.entry(key).or_insert_with(|| {
            let mut builder = QueryBuilder::<
                (Entity, &'static DespawnAtOutOfLog),
                With<SharedBuffer>,
            >::new(world);
            for component_id in components.iter().copied() {
                builder.without_id(component_id);
            }
            let query_state = builder.build();
            ComponentBufferData {
                query_state,
                components,
            }
        });
        Some(ComponentBuffer {
            key,
            entity,
            buffer: data.get_buffer(world),
            components_buffered: false,
        })
    }
}

#[derive(Resource, Default)]
pub(crate) struct ComponentBufferRes {
    data: ComponentBufferMap,
    cache: HashMap<u64, Option<u64>, PassHash>,
    ongoing_buffer: bool,
}

impl ComponentBufferRes {
    fn get_cached<I: IntoIterator<Item = ComponentId>>(
        &mut self,
        world: &mut World,
        entity: Entity,
        cache: impl Hash,
        components: impl FnOnce(&mut World) -> I,
    ) -> Option<ComponentBuffer> {
        let mut hasher = FixedHasher::default().build_hasher();
        cache.hash(&mut hasher);
        match self.cache.entry(hasher.finish()) {
            Entry::Occupied(occupied) => {
                let Some(key) = occupied.get().as_ref().copied() else {
                    return None;
                };
                let buffer = self.data.map.get_mut(&key).expect("todo").get_buffer(world);
                Some(ComponentBuffer {
                    key,
                    entity,
                    buffer,
                    components_buffered: false,
                })
            }
            Entry::Vacant(vacant) => {
                let components = components(world);
                let buffer = self.data.get(world, entity, components);
                vacant.insert(buffer.as_ref().map(|buffer| buffer.key));
                buffer
            }
        }
    }
}

#[derive(Debug)]
struct ComponentBuffer {
    key: u64,
    entity: Entity,
    buffer: Entity,
    components_buffered: bool,
}

impl ComponentBuffer {
    fn move_components(&mut self, world: &mut World) {
        world.resource_scope::<ComponentBufferRes, _>(|world, mut component_buffers| {
            let (source, target) = if self.components_buffered {
                (self.buffer, self.entity)
                // todo: leftover required components should be removed
            } else {
                (self.entity, self.buffer)
            };
            let components = component_buffers
                .data
                .map
                .get_mut(&self.key)
                .expect("todo")
                .components
                .iter()
                .copied();
            let mut cloner = EntityCloner::build(world);
            cloner
                .deny_all()
                .move_components(true)
                .without_required_components(|builder| {
                    builder.allow_by_ids(components);
                });
            component_buffers.ongoing_buffer = true;
            cloner.clone_entity(source, target);
            component_buffers.ongoing_buffer = false;
            self.components_buffered = !self.components_buffered;
        });
    }
}

impl UndoRedo for ComponentBuffer {
    fn undo(&mut self, world: &mut World) {
        self.move_components(world);
    }
    fn redo(&mut self, world: &mut World) {
        self.move_components(world);
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
    let component_id = entity_mut.world().component_id::<DespawnAtOutOfLog>().unwrap();
    if entity_mut.contains_id(component_id) {
        return false;
    }
    
    let at = entity_mut.get_resource::<RevMeta>().expect("todo").now();
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

#[macro_export]
macro_rules! unique_for_location {
    ($($hashable: ident),*) => {
        {
            struct __Private;
            (std::any::TypeId::of::<__Private>(), $($hashable,)*)
        }
    }
}

pub use unique_for_location;

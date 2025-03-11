use std::{
    alloc::Layout,
    collections::VecDeque,
    error::Error,
    fmt::{Debug, Display},
    hash::{BuildHasher, Hash, Hasher},
    iter::FusedIterator,
    ptr::NonNull,
    sync::Arc,
};

use bevy::{
    ecs::{
        archetype::{ArchetypeGeneration, ArchetypeId},
        component::{
            Component, ComponentCloneBehavior, ComponentDescriptor, ComponentId, HookContext,
            StorageType,
        },
        entity::{hash_set::EntityHashSet, Entity, EntityCloner},
        hierarchy::Children,
        resource::Resource,
        system::{Commands, EntityCommands},
        world::{DeferredWorld, EntityWorldMut, FromWorld, Mut, World},
    },
    log::error,
    platform_support::{
        collections::{HashMap, HashSet},
        hash::{FixedHasher, PassHash},
    },
    ptr::OwningPtr,
    utils::synccell::SyncCell,
};
use fixedbitset::FixedBitSet;

use crate::{
    log::{DenseTransitionsLog, FrameTransitionLog},
    meta::{RevDirection, RevMeta},
};

mod commands;
mod entity_commands;

#[cfg(test)]
mod test;

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

pub trait RevWorld {
    fn rev_despawn(&mut self, entity: Entity) -> bool;
    fn rev_buffer_components(
        &mut self,
        entity: Entity,
        components: impl IntoIterator<Item = ComponentId>,
    ) -> Option<Entity>;
    fn rev_buffer_components_cached<I: IntoIterator<Item = ComponentId>>(
        &mut self,
        entity: Entity,
        cache: impl Hash,
        components: impl FnOnce(&mut World) -> I,
    ) -> Option<Entity>;
    fn rev_buffer_components_at_undo(
        &mut self,
        entity: Entity,
        components: impl IntoIterator<Item = ComponentId>,
    ) -> Option<Entity>;
    fn rev_buffer_components_at_undo_cached<I: IntoIterator<Item = ComponentId>>(
        &mut self,
        entity: Entity,
        cache: impl Hash,
        components: impl FnOnce(&mut World) -> I,
    ) -> Option<Entity>;
    fn rev_buffer_components_moving(&self) -> bool;
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
    ) -> Option<Entity> {
        self.resource_scope::<ComponentBufferRes, _>(|world, mut component_buffers| {
            component_buffers
                .get_buffer(world, entity, components)
                .map(|buffer| buffer.move_components_and_buffer(world, component_buffers))
        })
    }
    fn rev_buffer_components_cached<I: IntoIterator<Item = ComponentId>>(
        &mut self,
        entity: Entity,
        cache: impl Hash,
        components: impl FnOnce(&mut World) -> I,
    ) -> Option<Entity> {
        self.resource_scope::<ComponentBufferRes, _>(|world, mut component_buffers| {
            component_buffers
                .get_buffer_cached(world, entity, cache, components)
                .map(|buffer| buffer.move_components_and_buffer(world, component_buffers))
        })
    }
    fn rev_buffer_components_at_undo(
        &mut self,
        entity: Entity,
        components: impl IntoIterator<Item = ComponentId>,
    ) -> Option<Entity> {
        self.resource_scope::<ComponentBufferRes, _>(|world, mut component_buffers| {
            component_buffers
                .get_buffer(world, entity, components)
                .map(|buffer| buffer.reserve_components_and_buffer(world, &component_buffers))
        })
    }
    fn rev_buffer_components_at_undo_cached<I: IntoIterator<Item = ComponentId>>(
        &mut self,
        entity: Entity,
        cache: impl Hash,
        components: impl FnOnce(&mut World) -> I,
    ) -> Option<Entity> {
        self.resource_scope::<ComponentBufferRes, _>(|world, mut component_buffers| {
            component_buffers
                .get_buffer_cached(world, entity, cache, components)
                .map(|buffer| buffer.reserve_components_and_buffer(world, &component_buffers))
        })
    }
    fn rev_buffer_components_moving(&self) -> bool {
        self.get_resource::<ComponentBufferRes>()
            .is_some_and(|component_buffers| component_buffers.ongoing_buffer)
    }
}

#[derive(Component, Clone, Copy, Debug, Hash, PartialOrd, Ord, PartialEq, Eq)]
#[component(immutable, on_insert = DespawnAtOutOfLog::on_insert)]
pub struct DespawnAtOutOfLog(u64);

impl DespawnAtOutOfLog {
    pub fn new(meta: &RevMeta) -> Self {
        assert_eq!(meta.get_direction(), Some(RevDirection::NOT_LOG));
        Self(meta.now())
    }
    pub fn added_at(self) -> u64 {
        self.0
    }
    fn on_insert(mut world: DeferredWorld, hook: HookContext) {
        let Some(meta) = world.get_resource::<RevMeta>() else {
            return;
        };
        let now = meta.now();
        if meta.get_direction() != Some(RevDirection::NOT_LOG) {
            return;
        }
        let Some(location) = world.entities().get(hook.entity) else {
            return;
        };
        let mut res = world.resource_mut::<ComponentBufferRes>();
        if res.buffered_in_archetype_at != now {
            res.buffered_in_archetype_at = now;
            res.archetypes_buffered_to_this_frame.clear();
        }
        res.archetypes_buffered_to_this_frame
            .insert(location.archetype_id.index());
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

struct ComponentBufferData {
    components: Box<[ComponentId]>,
    // todo: archetypes for undo and for redo, redo don't need absence of reservations?
    // unify getting bufer entity, automatically add components or reservations
    undo_reservations: Box<[ComponentId]>,
    unwanted_required: Box<[ComponentId]>,
    without_components: Vec<ArchetypeId>,
    moved_from: usize,
    generation: ArchetypeGeneration,
}

impl ComponentBufferData {
    fn get_buffer_entity(
        &mut self,
        world: &mut World,
        archetypes_buffered_to_this_frame: &FixedBitSet,
    ) -> Entity {
        let meta = world.get_resource::<RevMeta>().unwrap();
        let now_marker = DespawnAtOutOfLog::new(meta);
        let marker_id = world.component_id::<DespawnAtOutOfLog>().expect("todo");

        // update fitting archetypes with new ones if any
        self.without_components.extend(
            world.archetypes()[self.generation..]
                .iter()
                .filter(|archetype| {
                    archetype.contains(marker_id)
                        && self
                            .components
                            .iter()
                            .chain(&self.undo_reservations)
                            .all(|id| !archetype.contains(*id))
                })
                .map(|archetype| archetype.id()),
        );
        self.generation = world.archetypes().generation();

        // try find existing buffer entity, favoring archetypes that have been moved from already so no new archetypes are created
        let archetypes = self.without_components.iter().copied().enumerate();
        for (i, archetype_id) in archetypes {
            if !archetypes_buffered_to_this_frame.contains(archetype_id.index()) {
                continue;
            }
            let archetype = world.archetypes().get(archetype_id).unwrap();
            let table = world.storages().tables.get(archetype.table_id()).unwrap();
            for archetype_entity in archetype.entities() {
                let ptr = unsafe {
                    // SAFETY: this non-pub resource cannot have been transfered from the world it was created at to another
                    table.get_component(marker_id, archetype_entity.table_row())
                }
                .unwrap();
                let marker = unsafe {
                    // SAFETY: marker_id was just read from the world for this type
                    ptr.deref::<DespawnAtOutOfLog>()
                };
                if *marker == now_marker {
                    if i >= self.moved_from {
                        self.without_components.swap(i, self.moved_from);
                        self.moved_from += 1;
                    }
                    return archetype_entity.id();
                }
            }
        }

        // spawn a new buffer as no available has been found, the archetype here matches self.archetypes_without_components[0]
        world.spawn(now_marker).id()
    }
}

#[derive(Resource)]
pub(crate) struct ComponentBufferRes {
    buffers: HashMap<u64, ComponentBufferData, PassHash>,
    undo_reservations: HashMap<ComponentId, ComponentId>,
    unclonable: HashSet<ComponentId>,
    empty_with_marker: ArchetypeId,
    cache: HashMap<u64, Option<u64>, PassHash>,
    archetypes_buffered_to_this_frame: FixedBitSet,
    archetypes_buffered_from_any_frame: FixedBitSet,
    buffered_in_archetype_at: u64,
    ongoing_buffer: bool,
}

impl FromWorld for ComponentBufferRes {
    fn from_world(world: &mut World) -> Self {
        let entity = world.spawn(DespawnAtOutOfLog(0));
        let empty_with_marker = entity.archetype().id();
        entity.despawn();
        Self {
            buffers: HashMap::default(),
            undo_reservations: HashMap::default(),
            unclonable: HashSet::default(),
            empty_with_marker,
            cache: HashMap::default(),
            archetypes_buffered_to_this_frame: FixedBitSet::new(),
            archetypes_buffered_from_any_frame: FixedBitSet::new(),
            buffered_in_archetype_at: 0,
            ongoing_buffer: false,
        }
    }
}

impl ComponentBufferRes {
    fn get_buffer_cached<I: IntoIterator<Item = ComponentId>>(
        &mut self,
        world: &mut World,
        entity: Entity,
        cache: impl Hash,
        components: impl FnOnce(&mut World) -> I,
    ) -> Option<ComponentBuffer> {
        let cache = hash_cache(cache);
        if let Some(key) = self.cache.get(&cache).copied() {
            let key = key?;
            let buffer = self
                .buffers
                .get_mut(&key)
                .expect("todo")
                .get_buffer_entity(world, &self.archetypes_buffered_to_this_frame);
            Some(ComponentBuffer {
                key,
                entity,
                buffer,
                components_buffered: false,
            })
        } else {
            let components = components(world);
            let buffer = self.get_buffer(world, entity, components);
            let key = buffer.as_ref().map(|buffer| buffer.key);
            self.cache.insert(cache, key);
            buffer
        }
    }
    fn get_buffer(
        &mut self,
        world: &mut World,
        entity: Entity,
        components: impl IntoIterator<Item = ComponentId>,
    ) -> Option<ComponentBuffer> {
        let mut components = components
            .into_iter()
            .filter(|&component_id| {
                // todo: remove filter when linked issue is fixed
                let component_info = world.components().get_info(component_id).unwrap();
                let unclonable = component_info.clone_behavior() == &ComponentCloneBehavior::Ignore;
                if unclonable && self.unclonable.insert(component_id) {
                    error!(
                        "Unclonable component {} will be ignored by reversible structural operations, it's insert, remove \
                        or overwrite will not be reversible, see https://github.com/bevyengine/bevy/issues/18079",
                        component_info.name()
                    );
                }
                unclonable
            })
            .collect::<Box<[ComponentId]>>();

        if components.is_empty() {
            return None;
        }

        let mut hasher = FixedHasher::default().build_hasher();
        components.sort_unstable();
        for component_id in components.iter().copied() {
            component_id.hash(&mut hasher);
        }
        let key = hasher.finish();
        let data = self.buffers.entry(key).or_insert_with(|| {
            let undo_reservations = components
                .iter()
                .map(|&component_id| {
                    *self
                        .undo_reservations
                        .entry(component_id)
                        .or_insert_with(|| {
                            let descriptor = unsafe {
                                // SAFETY: (): Send + Sync + !Drop
                                ComponentDescriptor::new_with_layout(
                                    format!(
                                        "reservation to buffer {} ({component_id:?})",
                                        world.components().get_info(component_id).unwrap().name()
                                    ),
                                    StorageType::Table,
                                    Layout::new::<()>(),
                                    None, // !Drop
                                    false,
                                    ComponentCloneBehavior::Ignore,
                                )
                            };
                            world.register_component_with_descriptor(descriptor)
                        })
                })
                .collect::<Box<[ComponentId]>>();
            let mut unwanted_required = components
                .iter()
                .flat_map(|component_id| {
                    world
                        .components()
                        .get_info(*component_id)
                        .unwrap()
                        .required_components()
                        .iter_ids()
                })
                .collect::<HashSet<_>>();
            for component_id in components.iter() {
                unwanted_required.remove(component_id);
            }
            ComponentBufferData {
                components,
                undo_reservations,
                unwanted_required: unwanted_required.into_iter().collect(),
                without_components: vec![self.empty_with_marker],
                moved_from: 1,
                generation: world.archetypes().generation(),
            }
        });
        let buffer = data.get_buffer_entity(world, &self.archetypes_buffered_to_this_frame);
        Some(ComponentBuffer {
            key,
            entity,
            buffer,
            components_buffered: false,
        })
    }
    fn get_buffer_components(&self, cache: impl Hash) -> &[ComponentId] {
        let cache = hash_cache(cache);
        &self.buffers.get(&cache).unwrap().components
    }
}

fn hash_cache(cache: impl Hash) -> u64 {
    let mut hasher = FixedHasher::default().build_hasher();
    cache.hash(&mut hasher);
    hasher.finish()
}

#[derive(Debug)]
struct ComponentBuffer {
    key: u64,
    entity: Entity,
    buffer: Entity,
    components_buffered: bool,
}

impl ComponentBuffer {
    fn reserve_components_and_buffer(
        self,
        world: &mut World,
        buffer_data: &ComponentBufferRes,
    ) -> Entity {
        struct Reserved {
            key: u64,
            buffer: Entity,
        }

        impl UndoRedo for Reserved {
            fn undo(&mut self, world: &mut World) {
                world.resource_scope::<ComponentBufferRes, ()>(|world, buffer_data| {
                    let undo_reservations = &*buffer_data.buffers[&self.key].undo_reservations;
                    world
                        .entity_mut(self.buffer)
                        .remove_by_ids(undo_reservations);
                })
            }
            fn redo(&mut self, world: &mut World) {
                world.resource_scope::<ComponentBufferRes, ()>(|world, buffer_data| {
                    self.inner(world, &buffer_data);
                })
            }
        }

        impl Reserved {
            fn inner(&self, world: &mut World, buffer_data: &ComponentBufferRes) {
                let undo_reservations = &*buffer_data.buffers[&self.key].undo_reservations;
                let iter = std::iter::repeat_with(|| unsafe {
                    // SAFETY: () is a ZST which makes NonNull::dangling a valid pointer to read from regardless of lifetimes
                    OwningPtr::new(NonNull::dangling())
                })
                .take(undo_reservations.len());
                let mut buffer = world.entity_mut(self.buffer);
                unsafe {
                    // SAFETY: ids are registered in this world for () that the iterator yields OwningPtr of
                    buffer.insert_by_ids(undo_reservations, iter);
                }
            }
        }

        let buffer = self.buffer;
        let undo_redo = Reserved {
            key: self.key,
            buffer,
        };

        undo_redo.inner(world, buffer_data);
        world.buffer_undo_redo(undo_redo);
        world.buffer_undo_redo(self);
        buffer
    }
    fn move_components_and_buffer(
        mut self,
        world: &mut World,
        buffer_data: Mut<ComponentBufferRes>,
    ) -> Entity {
        let buffer = self.buffer;
        self.move_components(world, buffer_data);
        world.buffer_undo_redo(self);
        buffer
    }
    fn move_components(&mut self, world: &mut World, mut buffer_data: Mut<ComponentBufferRes>) {
        buffer_data.ongoing_buffer = true;
        let data = buffer_data.buffers.get(&self.key).expect("todo");
        let components = data.components.iter().copied();
        let move_components = |world: &mut World, source: Entity, target: Entity| {
            EntityCloner::build(world)
                .deny_all()
                .move_components(true)
                .without_required_components(|builder| {
                    builder.allow_by_ids(components);
                })
                .clone_entity(source, target);
        };
        if self.components_buffered {
            move_components(world, self.buffer, self.entity);
        } else if data.unwanted_required.is_empty() {
            move_components(world, self.entity, self.buffer);
        } else {
            let buffer = world.entity(self.buffer);
            let archetype = buffer.archetype();
            let unwanted_required = data
                .unwanted_required
                .iter()
                .copied()
                .filter(|component_id| !archetype.contains(*component_id))
                .collect::<Vec<ComponentId>>();
            move_components(world, self.entity, self.buffer);
            if !unwanted_required.is_empty() {
                world
                    .entity_mut(self.buffer)
                    .remove_by_ids(&unwanted_required);
            }
        }
        self.components_buffered = !self.components_buffered;
        buffer_data.ongoing_buffer = false;
    }
}

impl UndoRedo for ComponentBuffer {
    fn undo(&mut self, world: &mut World) {
        world.resource_scope(|world, buffer_data| {
            self.move_components(world, buffer_data);
        });
    }
    fn redo(&mut self, world: &mut World) {
        world.resource_scope(|world, buffer_data| {
            self.move_components(world, buffer_data);
        });
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
    let component_id = entity_mut
        .world()
        .component_id::<DespawnAtOutOfLog>()
        .unwrap();
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
            struct Private;
            (std::any::TypeId::of::<Private>(), $($hashable,)*)
        }
    }
}

pub use unique_for_location;

// new buffer variant with dyn bundles

mod buffer_dyn_bundle {
    use bevy::ecs::bundle::BundleId;

    use super::*;

    struct BufferQuery {
        without_components: BufferArchetypes,
        without_components_and_reservations: BufferArchetypes,
        reservations: BundleId,
        generation: ArchetypeGeneration,
    }

    struct BufferArchetypes {
        archetypes: Vec<ArchetypeId>,
        moved_from: usize,
    }

    #[derive(Resource)]
    pub(crate) struct ComponentBufferRes {
        buffers: HashMap<BundleId, BufferQuery>,
        reservations: HashMap<ComponentId, ComponentId>,
        unclonable: HashSet<ComponentId>,
        empty_with_marker: ArchetypeId,
        cache: HashMap<u64, Option<BundleId>, PassHash>,
        archetypes_buffered_to_this_frame: FixedBitSet,
        buffered_in_archetype_at: u64,
        ongoing_buffer: bool,
    }

    impl ComponentBufferRes {
        fn get_buffer_cached<I: IntoIterator<Item = ComponentId>>(
            &mut self,
            world: &mut World,
            entity: Entity,
            cache: impl Hash,
            move_now: bool,
            components: impl FnOnce(&mut World) -> I,
        ) -> Option<Entity> {
            let cache = hash_cache(cache);
            if let Some(key) = self.cache.get(&cache).copied() {
                let key = key?;
                let query = self
                    .buffers
                    .get_mut(&key)
                    .expect("todo");
                Some(query.apply_and_get_buffer(
                    key, 
                    world, 
                    entity,
                    &self.archetypes_buffered_to_this_frame, 
                    move_now
                ))
            } else {
                let components = components(world);
                match self.collect_components(world, components) {
                    Some((key, components)) => {
                        self.cache.insert(cache, Some(key));
                        Some(self.foo(
                            world,
                            entity,
                            move_now,
                            key,
                            components
                        ))
                    },
                    None => {
                        self.cache.insert(cache, None);
                        None
                    }
                }
            }
        }
        fn get_buffer(
            &mut self,
            world: &mut World,
            entity: Entity,
            move_now: bool,
            components: impl IntoIterator<Item = ComponentId>,
        ) -> Option<Entity> {
            self.collect_components(world, components).map(|(key, components)| {
                self.foo(
                    world,
                    entity,
                    move_now,
                    key,
                    components
                )
            })
        }
        fn foo(
            &mut self,
            world: &mut World,
            entity: Entity,
            move_now: bool,
            key: BundleId,
            components: Vec<ComponentId>
        ) -> Entity {
            let query = self.buffers.entry(key).or_insert_with(|| {
                BufferQuery::new(
                    world,
                    components,
                    &mut self.reservations,
                    self.empty_with_marker,
                )
            });
            let meta = world.get_resource::<RevMeta>().expect("todo");
            let now_marker = DespawnAtOutOfLog::new(meta);
            let marker_id = world.component_id::<DespawnAtOutOfLog>().expect("todo");
            query.extend_archetypes(world, key, marker_id);
            let buffer = if move_now {
                query.without_components.get_buffer_entity(
                    world,
                    &self.archetypes_buffered_to_this_frame,
                    now_marker,
                    marker_id,
                )
            } else {
                query.without_components_and_reservations.get_buffer_entity(
                    world,
                    &self.archetypes_buffered_to_this_frame,
                    now_marker,
                    marker_id,
                )
            };

            let buffer_entity = buffer.id();
            let mut undo_redo = ComponentBuffer {
                components: key,
                entity,
                buffer: buffer_entity,
                components_buffered: false
            };

            if move_now {
                undo_redo.move_components(world);
            } else {
                undo_redo.reserve_components(query.reservations, buffer);
            }
            world.buffer_undo_redo(undo_redo);
            buffer_entity
        }
        fn collect_components(
            &mut self,
            world: &mut World,
            components: impl IntoIterator<Item = ComponentId>,
        ) -> Option<(BundleId, Vec<ComponentId>)> {
            let components: Vec<ComponentId> = components
                .into_iter()
                .filter(|&component_id| {
                    // todo: remove filter when linked issue is fixed
                    let component_info = world.components().get_info(component_id).unwrap();
                    let unclonable = component_info.clone_behavior() == &ComponentCloneBehavior::Ignore;
                    if unclonable && self.unclonable.insert(component_id) {
                        error!(
                            "Unclonable component {} will be ignored by reversible structural operations, it's insert, remove \
                            or overwrite will not be reversible, see https://github.com/bevyengine/bevy/issues/18079",
                            component_info.name()
                        );
                    }
                    unclonable
                })
                .collect();

            if components.is_empty() {
                return None;
            }

            Some((world.register_dynamic_bundle(&components).id(), components))
        }
    }

    impl BufferQuery {
        fn new(
            world: &mut World,
            components: Vec<ComponentId>,
            reservations: &mut HashMap<ComponentId, ComponentId>,
            empty_with_marker: ArchetypeId,
        ) -> Self {
            let reservations: Vec<ComponentId> = components
                .into_iter()
                .map(|component_id| {
                    *reservations.entry(component_id).or_insert_with(|| {
                        let descriptor = unsafe {
                            // SAFETY: (): Send + Sync + !Drop
                            ComponentDescriptor::new_with_layout(
                                format!(
                                    "reservation to buffer {} ({component_id:?})",
                                    world.components().get_info(component_id).unwrap().name()
                                ),
                                StorageType::Table,
                                Layout::new::<()>(),
                                None, // !Drop
                                false,
                                ComponentCloneBehavior::Ignore,
                            )
                        };
                        world.register_component_with_descriptor(descriptor)
                    })
                })
                .collect();
            let reservations = world.register_dynamic_bundle(&reservations).id();
            Self {
                reservations,
                without_components: BufferArchetypes {
                    archetypes: vec![empty_with_marker],
                    moved_from: 1,
                },
                without_components_and_reservations: BufferArchetypes {
                    archetypes: Vec::new(),
                    moved_from: 0,
                },
                generation: ArchetypeGeneration::initial(),
            }
        }
        fn get_buffer_entity<'a>(
            &mut self,
            key: BundleId,
            world: &'a mut World,
            archetypes_buffered_to_this_frame: &FixedBitSet,
            move_now: bool,
        ) -> EntityWorldMut<'a> {
            let meta = world.get_resource::<RevMeta>().expect("todo");
            let now_marker = DespawnAtOutOfLog::new(meta);
            let marker_id = world.component_id::<DespawnAtOutOfLog>().expect("todo");
            self.extend_archetypes(world, key, marker_id);
            if move_now {
                self.without_components.get_buffer_entity(
                    world,
                    archetypes_buffered_to_this_frame,
                    now_marker,
                    marker_id,
                )
            } else {
                self.without_components_and_reservations.get_buffer_entity(
                    world,
                    archetypes_buffered_to_this_frame,
                    now_marker,
                    marker_id,
                )
            }
        }
        fn apply_and_get_buffer(
            &mut self,
            key: BundleId,
            world: &mut World,
            entity: Entity,
            archetypes_buffered_to_this_frame: &FixedBitSet,
            move_now: bool,
        ) -> Entity {
            let meta = world.get_resource::<RevMeta>().expect("todo");
            let now_marker = DespawnAtOutOfLog::new(meta);
            let marker_id = world.component_id::<DespawnAtOutOfLog>().expect("todo");
            self.extend_archetypes(world, key, marker_id);
            let buffer = if move_now {
                self.without_components.get_buffer_entity(
                    world,
                    archetypes_buffered_to_this_frame,
                    now_marker,
                    marker_id,
                )
            } else {
                self.without_components_and_reservations.get_buffer_entity(
                    world,
                    archetypes_buffered_to_this_frame,
                    now_marker,
                    marker_id,
                )
            };

            let buffer_entity = buffer.id();
            let mut undo_redo = ComponentBuffer {
                components: key,
                entity,
                buffer: buffer_entity,
                components_buffered: false
            };

            if move_now {
                undo_redo.move_components(world);
            } else {
                undo_redo.reserve_components(self.reservations, buffer);
            }
            world.buffer_undo_redo(undo_redo);
            buffer_entity
        }
        fn extend_archetypes(&mut self, world: &World, key: BundleId, marker_id: ComponentId) {
            if world.archetypes().generation() == self.generation {
                return;
            }

            let get_ids = |id| world.bundles().get(id).expect("todo").explicit_components();
            let components = get_ids(key);
            let reservations = get_ids(self.reservations);

            for archetype in &world.archetypes()[self.generation..] {
                if archetype.contains(marker_id)
                    && components.iter().all(|id| !archetype.contains(*id))
                {
                    self.without_components.archetypes.push(archetype.id());
                    if reservations.iter().all(|id| !archetype.contains(*id)) {
                        self.without_components_and_reservations
                            .archetypes
                            .push(archetype.id());
                    }
                }
            }

            self.generation = world.archetypes().generation();
        }
    }

    impl BufferArchetypes {
        fn get_buffer_entity<'a>(
            &mut self,
            world: &'a mut World,
            archetypes_buffered_to_this_frame: &FixedBitSet,
            now_marker: DespawnAtOutOfLog,
            marker_id: ComponentId,
        ) -> EntityWorldMut<'a> {
            let archetypes = self.archetypes.iter().copied().enumerate();
            for (i, archetype_id) in archetypes {
                if !archetypes_buffered_to_this_frame.contains(archetype_id.index()) {
                    continue;
                }
                let archetype = world.archetypes().get(archetype_id).expect("todo");
                let table = world
                    .storages()
                    .tables
                    .get(archetype.table_id())
                    .expect("todo");
                for archetype_entity in archetype.entities() {
                    let ptr = unsafe {
                        // SAFETY: this non-pub resource cannot have been transfered from the world it was created at to another
                        table.get_component(marker_id, archetype_entity.table_row())
                    }
                    .expect("todo");
                    let marker = unsafe {
                        // SAFETY: marker_id was just read from the world for this type
                        ptr.deref::<DespawnAtOutOfLog>()
                    };
                    if *marker == now_marker {
                        if i >= self.moved_from {
                            self.archetypes.swap(i, self.moved_from);
                            self.moved_from += 1;
                        }
                        return world.entity_mut(archetype_entity.id());
                    }
                }
            }

            // spawn a new buffer as no available has been found, the archetype here matches self.archetypes_without_components[0]
            world.spawn(now_marker)
        }
    }
    
    #[derive(Debug)]
    struct ComponentBuffer {
        components: BundleId,
        entity: Entity,
        buffer: Entity,
        components_buffered: bool,
    }

    impl ComponentBuffer {
        fn move_components(&mut self, world: &mut World) {
            fn move_components(world: &mut World, components: &[ComponentId], source: Entity, target: Entity) {
                EntityCloner::build(world)
                    .deny_all()
                    .move_components(true)
                    .without_required_components(|builder| {
                        builder.allow_by_ids(components.iter().copied());
                    })
                    .clone_entity(source, target);
            }

            let bundle = world
                .bundles()
                .get(self.components)
                .expect("todo");
            let components: Box<[ComponentId]> = bundle.explicit_components().into();

            if self.components_buffered {
                move_components(world, &components, self.buffer, self.entity);
            } else if bundle.required_components().is_empty() {
                move_components(world, &components, self.entity, self.buffer);
            } else {
                let archetype_id = world.entities().get(self.buffer).expect("todo").archetype_id;
                let archetype = world.archetypes().get(archetype_id).expect("todo");
                let unwanted: Vec<ComponentId> = bundle
                    .required_components()
                    .iter()
                    .copied()
                    .filter(|component_id| !archetype.contains(*component_id))
                    .collect();
                move_components(world, &components, self.entity, self.buffer);
                if !unwanted.is_empty() {
                    world
                        .entity_mut(self.buffer)
                        .remove_by_ids(&unwanted);
                }
            }
            self.components_buffered = !self.components_buffered;
        }

        fn reserve_components(
            &self,
            reservations: BundleId,
            mut buffer: EntityWorldMut
        ) {
            
            struct Reserved {
                reservations: BundleId,
                buffer: Entity,
            }

            impl UndoRedo for Reserved {
                fn undo(&mut self, world: &mut World) {
                    let component_ids = self.component_ids(&world);
                    world
                        .entity_mut(self.buffer)
                        .remove_by_ids(&component_ids);
                }
                fn redo(&mut self, world: &mut World) {
                    self.redo_inner(&mut world.entity_mut(self.buffer));
                }
            }

            impl Reserved {
                fn component_ids(&self, world: &World) -> Box<[ComponentId]> {
                    world
                        .bundles()
                        .get(self.reservations)
                        .expect("todo")
                        .explicit_components()
                        .into()
                }
                fn redo_inner(&self, buffer: &mut EntityWorldMut) {
                    let component_ids = self.component_ids(buffer.world());
                    let iter = std::iter::repeat_with(|| unsafe {
                        // SAFETY: () is a ZST which makes NonNull::dangling a valid pointer to read from regardless of lifetimes
                        OwningPtr::new(NonNull::dangling())
                    })
                    .take(component_ids.len());
                    unsafe {
                        // SAFETY: ids are registered in this world for () that the iterator yields OwningPtr of
                        buffer.insert_by_ids(&component_ids, iter);
                    }
                }
            }

            let undo_redo = Reserved {
                reservations,
                buffer: buffer.id(),
            };

            undo_redo.redo_inner(&mut buffer);
            buffer.buffer_undo_redo(undo_redo);
        }
    }

    impl UndoRedo for ComponentBuffer {
        fn undo(&mut self, world: &mut World) {
            world.resource_scope(|world, mut buffer_data: Mut<ComponentBufferRes>| {
                buffer_data.ongoing_buffer = true;
                self.move_components(world);
                buffer_data.ongoing_buffer = false;
            });
        }
        fn redo(&mut self, world: &mut World) {
            self.undo(world);
        }
    }
}

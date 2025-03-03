use std::{
    any::TypeId,
    collections::VecDeque,
    error::Error,
    fmt::{Debug, Display},
    hash::{BuildHasher, Hash, Hasher},
};

use bevy::{
    ecs::{
        archetype::{self, Archetype, ArchetypeId},
        bundle::{Bundle, BundleInfo},
        component::{require, Component, ComponentId},
        entity::{Entity, EntityCloner},
        query::{QueryBuilder, QueryState, With},
        resource::Resource,
        system::{Commands, EntityCommands},
        world::{DeferredWorld, EntityWorldMut, FromWorld, World},
    },
    platform_support::{
        collections::{hash_map::Entry, HashMap, HashSet},
        hash::{FixedHasher, PassHash},
    },
    reflect::Reflect,
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

/// Marker component that disables entities like [`Disabled`].
///
/// Entities marked with this component will be despawned when the relevant reversible command, for example
/// an undone [`rev_spawn_batch`], is in the state that added this marker and will be finalized by either the
/// [`RevMeta::run_rev_update`] system or [`RevBuffers::finalize_all`].
///
/// Until then entities with this marker should be considered as non-existing.
#[derive(Component, Default, Reflect, Debug)]
pub struct RevDisabled;

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

#[derive(Component, Clone, Copy, Debug, Hash, PartialOrd, Ord, PartialEq, Eq)]
#[component(immutable)]
#[require(RevDisabled)]
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

#[derive(Resource)]
struct EmptyEntity(Entity);

impl FromWorld for EmptyEntity {
    fn from_world(world: &mut World) -> Self {
        // do not just take any existing empty entity, it likely belongs to other logic
        Self(world.spawn_empty().id())
    }
}
pub trait EmptyEntityScope {
    /// Get a scope of the world and an entity id that is empty.
    ///
    /// When the closure returns, the entity is cleared from any remaining components.
    ///
    /// The entity may be changed, for example when components are moved into it and another
    /// entity became empty. One should not otherwise put any assumptions on the entity
    /// after returning as other code calling this method may change the empty entity as well.
    fn empty_entity_scope<Out>(&mut self, c: impl FnOnce(&mut Self, &mut Entity) -> Out) -> Out;
}

impl EmptyEntityScope for World {
    fn empty_entity_scope<Out>(&mut self, c: impl FnOnce(&mut Self, &mut Entity) -> Out) -> Out {
        self.init_resource::<EmptyEntity>();
        self.resource_scope::<EmptyEntity, Out>(|world, mut entity| {
            let out = c(world, &mut entity.0);
            world.entity_mut(entity.0).clear();
            out
        })
    }
}

fn archetype_insert_keep(bundle_info: &BundleInfo, archetype: &Archetype) -> Box<[ComponentId]> {
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

fn archetype_insert_replace_remove(
    bundle_info: &BundleInfo,
    archetype: &Archetype,
) -> Box<[ComponentId]> {
    bundle_info
        .iter_explicit_components()
        .filter(|component_id| archetype.contains(*component_id))
        .collect()
}

fn archetype_replace_with_requires(
    bundle_info: &BundleInfo,
    archetype: &Archetype,
) -> Box<[ComponentId]> {
    bundle_info
        .iter_contributed_components()
        .filter(|component_id| archetype.contains(*component_id))
        .collect()
}

#[derive(PartialEq, Eq, Hash)]
struct ReplaceComponents<Insert = [ComponentId; 1], Backup = [ComponentId; 1]> {
    insert: Insert,
    backup: Backup,
}

impl<Insert, Backup> ReplaceComponents<Insert, Backup>
where
    for<'a> &'a Insert: IntoIterator<Item = &'a ComponentId>,
    for<'a> &'a Backup: IntoIterator<Item = &'a ComponentId>,
{
    fn movers<const UNDO: bool>(&self, world: &mut World) -> (EntityCloner, EntityCloner) {
        if UNDO {
            (
                move_components(world, (&self.insert).into_iter().copied(), true),
                move_components(world, (&self.backup).into_iter().copied(), true),
            )
        } else {
            (
                move_components(world, (&self.backup).into_iter().copied(), true),
                move_components(world, (&self.insert).into_iter().copied(), true),
            )
        }
    }
}

fn move_components(
    world: &mut World,
    components: impl Iterator<Item = ComponentId>,
    with_despawn_at_out_of_log: bool,
) -> EntityCloner {
    let mut builder = EntityCloner::build(world);
    builder
        .deny_all()
        .without_required_components(move |builder| {
            builder.allow_by_ids(components);
            if with_despawn_at_out_of_log {
                builder.allow::<DespawnAtOutOfLog>();
            }
        })
        .move_components(true);
    builder.finish()
}

#[derive(Component, Clone, Copy)]
#[component(immutable)]
#[require(RevDisabled)]
pub struct SharedBuffer;

struct ComponentBufferData {
    query_state:
        QueryState<(Entity, &'static DespawnAtOutOfLog), (With<SharedBuffer>, With<RevDisabled>)>,
    components: Box<[ComponentId]>,
}

impl ComponentBufferData {
    fn move_components(
        &self,
        world: &mut World,
        source_target_pairs: impl IntoIterator<Item = (Entity, Entity)>,
    ) {
        let mut cloner = EntityCloner::build(world);
        let cloner = cloner
            .deny_all()
            .move_components(true)
            .without_required_components(|builder| {
                builder.allow_by_ids(self.components.iter().copied());
            });
        for (source, target) in source_target_pairs.into_iter() {
            cloner.clone_entity(source, target);
        }
    }
    fn get_buffer(&mut self, world: &mut World) -> Entity {
        let now = world.resource::<RevMeta>().now();
        let bundle = (DespawnAtOutOfLog::new(now), SharedBuffer);
        self
            .query_state
            .iter(&world)
            .filter(|(_, marker)| marker.added_at() == now)
            .map(|(entity, _)| entity)
            .next()
            .unwrap_or_else(|| world.spawn(bundle).id())
    }
}

#[derive(Default)]
struct ComponentBufferMap(HashMap<u64, ComponentBufferData, PassHash>);

impl ComponentBufferMap {
    fn noop_key() -> u64 {
        FixedHasher::default().build_hasher().finish()
    }
    fn without_ids(
        &mut self,
        world: &mut World,
        mut components: Box<[ComponentId]>,
        entity: Entity,
    ) -> ComponentBuffer {
        components.sort();
        let mut hasher = FixedHasher::default().build_hasher();
        for component_id in components.iter().copied() {
            component_id.hash(&mut hasher);
        }
        let key = hasher.finish();
        let data = self.0.entry(key).or_insert_with(|| {
            let mut builder = QueryBuilder::<
                (Entity, &'static DespawnAtOutOfLog),
                (With<SharedBuffer>, With<RevDisabled>),
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
        ComponentBuffer {
            key,
            entity,
            buffer: data.get_buffer(world),
            components_buffered: false,
        }
    }
}

#[derive(Resource, Default)]
pub(crate) struct ComponentBufferRes {
    data: ComponentBufferMap,
    cache: HashMap<u64, u64, PassHash>,
}

impl ComponentBufferRes {
    fn get(&self, key: u64) -> &ComponentBufferData {
        self.data.0.get(&key).expect("todo")
    }
    fn get_mut(&mut self, key: u64) -> &mut ComponentBufferData {
        self.data.0.get_mut(&key).expect("todo")
    }
    fn without_id(
        world: &mut World,
        without_requires: bool,
        component: ComponentId,
        entity: Entity,
    ) -> ComponentBuffer {
        if !without_requires {
            return Self::without_ids(world, false, [component], entity);
        }
        struct IdAndRequires;
        let archetype_id = world.entities().get(entity).unwrap().archetype_id;
        world.resource_scope::<ComponentBufferRes, ComponentBuffer>(|world, mut interner| {
            interner.without_ids_by_cache_inner::<IdAndRequires>(
                component,
                world,
                |world| {
                    let archetype = world.archetypes().get(archetype_id).unwrap();
                    world
                        .components()
                        .get_info(component)
                        .expect("todo")
                        .required_components()
                        .iter_ids()
                        .chain([component])
                        .filter(|component_id| archetype.contains(*component_id))
                        .collect()
                },
                entity,
                archetype_id
            )
        })
    }
    fn without_ids(
        world: &mut World,
        without_requires: bool,
        components: impl IntoIterator<Item = ComponentId>,
        entity: Entity,
    ) -> ComponentBuffer {
        let archetype_id = world.entities().get(entity).unwrap().archetype_id;
        let components = if without_requires {
            components_and_requires(world, components, archetype_id)
        } else {
            let archetype = world.archetypes().get(archetype_id).unwrap();
            components.into_iter().filter(|component_id| archetype.contains(*component_id)).collect()
        };
        world.resource_scope::<ComponentBufferRes, ComponentBuffer>(|world, mut interner| {
            interner.data.without_ids(world, components, entity)
        })
    }
    fn without_ids_by_cache_value<K: Hash + 'static>(
        world: &mut World,
        cache: K,
        components: impl FnOnce(&mut World, ArchetypeId) -> Box<[ComponentId]>,
        entity: Entity
    ) -> ComponentBuffer {
        world.resource_scope::<ComponentBufferRes, ComponentBuffer>(|world, mut interner| {
            let archetype_id = world.entities().get(entity).unwrap().archetype_id;
            let components = |world: &mut World| components(world, archetype_id);
            interner.without_ids_by_cache_inner::<K>(cache, world, components, entity, archetype_id)
        })
    }
    fn without_ids_by_cache_type<Unique: 'static>(
        world: &mut World,
        components: impl FnOnce(&mut World, ArchetypeId) -> Box<[ComponentId]>,
        entity: Entity
    ) -> ComponentBuffer {
        world.resource_scope::<ComponentBufferRes, _>(|world, mut interner| {
            let archetype_id = world.entities().get(entity).unwrap().archetype_id;
            let components = |world: &mut World| components(world, archetype_id);
            interner.without_ids_by_cache_inner::<Unique>((), world, components, entity, archetype_id)
        })
    }
    fn without_ids_by_cache_inner<Unique: 'static>(
        &mut self,
        cache: impl Hash,
        world: &mut World,
        components: impl FnOnce(&mut World) -> Box<[ComponentId]>,
        entity: Entity,
        archetype_id: ArchetypeId
    ) -> ComponentBuffer {
        let mut hasher = FixedHasher::default().build_hasher();
        (archetype_id, TypeId::of::<Unique>(), cache).hash(&mut hasher);
        match self.cache.entry(hasher.finish()) {
            Entry::Occupied(occupied) => {
                let key = *occupied.get();
                let buffer = self.get_mut(key).get_buffer(world);
                ComponentBuffer {
                    key,
                    entity,
                    buffer,
                    components_buffered: false
                }
            }
            Entry::Vacant(vacant) => {
                let components = components(world);
                let buffer = self
                    .data
                    .without_ids(world, components, entity);
                vacant.insert(buffer.key);
                buffer
            }
        }
    }
    fn without_bundle<T: Bundle>(
        world: &mut World,
        without_requires: bool,
        entity: Entity
    ) -> ComponentBuffer {
        if without_requires {
            struct Contributed;
            Self::without_ids_by_cache_type::<(T, Contributed), Box<[ComponentId]>, Out>(
                world,
                false,
                |world| world.register_bundle::<T>().contributed_components().into(),
                c,
            )
        } else {
            struct Explicit;
            Self::without_ids_by_cache_type::<(T, Explicit), Box<[ComponentId]>, Out>(
                world,
                false,
                |world| world.register_bundle::<T>().explicit_components().into(),
                c,
            )
        }
    }
}

fn components_and_requires(
    world: &World,
    components: impl IntoIterator<Item = ComponentId>,
    archetype_id: ArchetypeId
) -> Box<[ComponentId]> {
    let archetype = world.archetypes().get(archetype_id).unwrap();
    components
        .into_iter()
        .flat_map(|id| {
            world
                .components()
                .get_info(id)
                .expect("todo")
                .required_components()
                .iter_ids()
                .chain([id])
        })
        .filter(|component_id| archetype.contains(*component_id))
        .collect::<HashSet<ComponentId>>()
        .into_iter()
        .collect()
}

#[derive(Debug)]
pub struct ComponentBuffer {
    key: u64,
    entity: Entity,
    buffer: Entity,
    components_buffered: bool,
}

impl ComponentBuffer {
    pub fn without_id(
        world: &mut World,
        without_requires: bool,
        component: ComponentId,
        entity: Entity,
    ) -> Self {
        ComponentBufferRes::without_id(world, without_requires, component, Self::new(entity))
    }
    pub fn without_ids(
        world: &mut World,
        without_requires: bool,
        components: impl IntoIterator<Item = ComponentId>,
        entity: Entity,
    ) -> Self {
        ComponentBufferRes::without_ids(world, without_requires, components, Self::new(entity))
    }
    pub fn without_ids_by_cache_value<K: Hash + 'static, I: IntoIterator<Item = ComponentId>>(
        world: &mut World,
        without_requires: bool,
        cache: K,
        components: impl for<'a> FnOnce(&'a mut World) -> impl Iterator<Item = ComponentId> + 'a,
        entity: Entity,
    ) -> Self {
        ComponentBufferRes::without_ids_by_cache_value(
            world,
            without_requires,
            cache,
            components,
            Self::new(entity),
        )
    }
    pub fn without_ids_by_cache_type<Unique: 'static, I: IntoIterator<Item = ComponentId>>(
        world: &mut World,
        without_requires: bool,
        components: impl FnOnce(&mut World) -> I,
        entity: Entity,
    ) -> Self {
        ComponentBufferRes::without_ids_by_cache_type::<Unique, I, Self>(
            world,
            without_requires,
            components,
            Self::new(entity),
        )
    }
    pub fn without_bundle<T: Bundle>(
        world: &mut World,
        without_requires: bool,
        entity: Entity,
    ) -> Self {
        ComponentBufferRes::without_bundle::<T, Self>(world, without_requires, Self::new(entity))
    }
    fn new(entity: Entity) -> impl FnOnce(&mut World, u64, &mut ComponentBufferData) -> Self {
        move |world: &mut World, key: u64, data: &mut ComponentBufferData| {
            let now = world.resource::<RevMeta>().now();
            let bundle = (DespawnAtOutOfLog::new(now), SharedBuffer);
            let existing = data
                .query_state
                .iter(&world)
                .filter(|(_, marker)| marker.added_at() == now)
                .map(|(entity, _)| entity)
                .next();
            let buffer = existing.unwrap_or_else(|| world.spawn(bundle).id());
            Self {
                key,
                entity,
                buffer,
                components_buffered: false,
            }
        }
    }
    pub fn is_noop(&self) -> bool {
        self.key == ComponentBufferMap::noop_key()
    }
    pub fn move_components(&mut self, world: &mut World) {
        let source_target = if self.components_buffered {
            [(self.buffer, self.entity)]
        } else {
            [(self.entity, self.buffer)]
        };
        world.resource_scope::<ComponentBufferRes, ()>(|world, mut interner| {
            interner
                .get_mut(self.key)
                .move_components(world, source_target);
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

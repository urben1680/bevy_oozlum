use std::{
    any::TypeId,
    collections::VecDeque,
    error::Error,
    fmt::{Debug, Display},
    hash::{BuildHasher, Hash, Hasher},
};

use bevy::{
    ecs::{
        archetype::Archetype,
        bundle::{Bundle, BundleInfo},
        component::{require, Component, ComponentId},
        entity::{Entity, EntityCloner},
        query::{QueryBuilder, QueryState, With},
        resource::Resource,
        system::{Commands, EntityCommands},
        world::{DeferredWorld, EntityWorldMut, FromWorld, World},
    },
    platform_support::{
        collections::{HashMap, HashSet},
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

struct BufferData {
    query_state:
        QueryState<(Entity, &'static DespawnAtOutOfLog), (With<SharedBuffer>, With<RevDisabled>)>,
    cloner: SyncCell<EntityCloner>,
}

#[derive(Default)]
struct SharedBufferInternerMap(HashMap<InternedSharedBuffers, BufferData, PassHash>);

impl SharedBufferInternerMap {
    fn without_ids(
        &mut self,
        world: &mut World,
        components: impl IntoIterator<Item = ComponentId>,
    ) -> InternedSharedBuffers {
        let mut components: Box<[ComponentId]> = components.into_iter().collect();
        components.sort();
        let mut hasher = FixedHasher::default().build_hasher();
        for component in components.iter().copied() {
            component.hash(&mut hasher);
        }
        let key = InternedSharedBuffers(hasher.finish());
        self.0.entry(key).or_insert_with(|| {
            // build query state to find entities lacking `components`
            let mut builder = QueryBuilder::<
                (Entity, &'static DespawnAtOutOfLog),
                (With<SharedBuffer>, With<RevDisabled>),
            >::new(world);
            for component_id in components.iter().copied() {
                builder.without_id(component_id);
            }
            let query_state = builder.build();

            // build cloner to move `components`
            // todo: assert component to be clonable
            let mut builder = EntityCloner::build(world);
            builder.deny_all();
            builder.without_required_components(|builder| {
                builder.allow_by_ids(components);
            });
            builder.move_components(true);
            let cloner = SyncCell::new(builder.finish());
            BufferData {
                query_state,
                cloner,
            }
        });
        key
    }
}

#[derive(Resource, Default)]
struct SharedBufferInterner {
    query_states: SharedBufferInternerMap,
    lookup: HashMap<u64, InternedSharedBuffers, PassHash>,
}

impl SharedBufferInterner {
    fn without_ids_by_key<Unique: 'static, I: IntoIterator<Item = ComponentId>>(
        &mut self,
        key: impl Hash,
        world: &mut World,
        components: impl FnOnce(&mut World) -> I,
    ) -> InternedSharedBuffers {
        let mut hasher = FixedHasher::default().build_hasher();
        TypeId::of::<Unique>().hash(&mut hasher);
        key.hash(&mut hasher);
        *self.lookup.entry(hasher.finish()).or_insert_with(|| {
            let components = components(world);
            self.query_states.without_ids(world, components)
        })
    }
}

/// An interned [`QueryState`] to find entities that contain the following components:
/// - [`DespawnAtOutOfLog`]
/// - [`SharedBuffer`]
/// - [`RevDisabled`]
///
/// ... while not containing other components as specified during this struct's construction.
///
/// If no such entities exist in the desired amount, the missing entities are spawned.
///
/// These entities are used to temporarily store components, for which the returned [`Entity`]
/// should be saved to find them again, usually to undo the move into these buffer entities.
///
/// These entities can be stripped off [`SharedBuffer`] to make them not findable by this query
/// anymore. This may be useful if one wants to store the returned [`Entity`] longer than
/// until the time when the components are moved out of it again. Then it is ensured no
/// other part of the code reinserts these components into this entity later, always leaving
/// space for redoing the moves into the buffer.
///
/// Some constructors further reduce the lookup work into the interner resource by using
/// alternative mapping resources in between, for example by storing the [`TypeId`] in the
/// bundle constructors so the related components do not need to be hashed at every
/// following construction for the same bundle.
///
/// The [`without_ids`](Self::without_ids) and
/// [`without_ids_and_requires`](Self::without_ids_and_requires) constructors do not
/// do that so custom mapping resources for faster lookups may be benefitial.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct InternedSharedBuffers(u64);

impl InternedSharedBuffers {
    pub fn without_id(world: &mut World, without_requires: bool, component: ComponentId) -> Self {
        if !without_requires {
            return Self::without_ids(world, false, [component]);
        }
        struct IdAndRequires;
        world.resource_scope::<SharedBufferInterner, Self>(|world, mut interner| {
            interner.without_ids_by_key::<IdAndRequires, _>(component, world, |world| {
                world
                    .components()
                    .get_info(component)
                    .expect("todo")
                    .required_components()
                    .iter_ids()
                    .chain([component])
                    .collect::<Vec<ComponentId>>()
            })
        })
    }
    pub fn without_ids(
        world: &mut World,
        without_requires: bool,
        components: impl IntoIterator<Item = ComponentId>,
    ) -> Self {
        if !without_requires {
            return world.resource_scope::<SharedBufferInterner, Self>(|world, mut interner| {
                interner.query_states.without_ids(world, components)
            });
        }
        let components = components_and_requires(&world, components);
        Self::without_ids(world, false, components)
    }
    pub fn without_ids_by_key_value<K: Hash + 'static, I: IntoIterator<Item = ComponentId>>(
        world: &mut World,
        without_requires: bool,
        key: K,
        components: impl FnOnce(&mut World) -> I,
    ) -> Self {
        world.resource_scope::<SharedBufferInterner, Self>(|world, mut interner| {
            if !without_requires {
                return interner.without_ids_by_key::<K, _>(key, world, components);
            }
            interner.without_ids_by_key::<K, _>(key, world, |world| {
                let components = components(world);
                components_and_requires(&world, components)
            })
        })
    }
    pub fn without_ids_by_key_type<Unique: 'static, I: IntoIterator<Item = ComponentId>>(
        world: &mut World,
        without_requires: bool,
        components: impl FnOnce(&mut World) -> I,
    ) -> Self {
        world.resource_scope::<SharedBufferInterner, Self>(|world, mut interner| {
            if !without_requires {
                return interner.without_ids_by_key::<Unique, _>((), world, components);
            }
            interner.without_ids_by_key::<Unique, _>((), world, |world| {
                let components = components(world);
                components_and_requires(&world, components)
            })
        })
    }
    pub fn without_bundle<T: Bundle>(world: &mut World, without_requires: bool) -> Self {
        if without_requires {
            struct Contributed;
            Self::without_ids_by_key_type::<(T, Contributed), Box<[ComponentId]>>(
                world,
                false,
                |world| world.register_bundle::<T>().contributed_components().into(),
            )
        } else {
            struct Explicit;
            Self::without_ids_by_key_type::<(T, Explicit), Box<[ComponentId]>>(
                world,
                false,
                |world| world.register_bundle::<T>().explicit_components().into(),
            )
        }
    }
    pub fn get_entity(self, world: &mut World) -> Entity {
        let now = world.get_resource::<RevMeta>().expect("todo").now();
        self.get_entity_log(world, now)
    }
    pub fn get_entity_log(self, world: &mut World, logged_at: u64) -> Entity {
        world.resource_scope::<SharedBufferInterner, Entity>(|world, mut interner| {
            let existing = interner
                .query_states
                .0
                .get_mut(&self)
                .expect("todo")
                .query_state
                .iter(&world)
                .filter(|(_, marker)| marker.added_at() == logged_at)
                .map(|(entity, _)| entity)
                .next();
            let bundle = (DespawnAtOutOfLog::new(logged_at), SharedBuffer);
            existing.unwrap_or_else(|| world.spawn(bundle).id())
        })
    }
    pub fn get_entities(self, world: &mut World, amount: usize) -> Vec<Entity> {
        let now = world.get_resource::<RevMeta>().expect("todo").now();
        self.get_entities_log(world, amount, now)
    }
    pub fn get_entities_log(self, world: &mut World, amount: usize, logged_at: u64) -> Vec<Entity> {
        world.resource_scope::<SharedBufferInterner, Vec<Entity>>(|world, mut interner| {
            let existing = interner
                .query_states
                .0
                .get_mut(&self)
                .expect("todo")
                .query_state
                .iter(&world)
                .filter(|(_, marker)| marker.added_at() == logged_at)
                .map(|(entity, _)| entity)
                .take(amount);
            let mut entities = Vec::with_capacity(amount);
            entities.extend(existing);
            let remaining = amount - entities.len();
            let bundle = (DespawnAtOutOfLog::new(logged_at), SharedBuffer);
            entities.extend(world.spawn_batch(std::iter::repeat(bundle).take(remaining)));
            entities
        })
    }
    pub fn move_from_buffer(self, world: &mut World, buffer: Entity, target: Entity) {
        world.resource_scope::<SharedBufferInterner, ()>(|world, mut interner| {
            interner
                .query_states
                .0
                .get_mut(&self)
                .expect("todo")
                .cloner
                .get()
                .clone_entity(world, buffer, target);
        })
    }
    pub fn move_to_buffer(self, world: &mut World, source: Entity) -> Entity {
        let now = world.get_resource::<RevMeta>().expect("todo").now();
        self.move_to_buffer_log(world, source, now)
    }
    pub fn move_to_buffer_log(self, world: &mut World, source: Entity, logged_at: u64) -> Entity {
        let bundle = (DespawnAtOutOfLog::new(logged_at), SharedBuffer);
        world.resource_scope::<SharedBufferInterner, Entity>(|world, mut interner| {
            let data = interner.query_states.0.get_mut(&self).expect("todo");
            let existing = data
                .query_state
                .iter(&world)
                .filter(|(_, marker)| marker.added_at() == logged_at)
                .map(|(entity, _)| entity)
                .next();
            let target = existing.unwrap_or_else(|| world.spawn(bundle).id());
            data.cloner.get().clone_entity(world, source, target);
            target
        })
    }
}

fn components_and_requires(
    world: &World,
    components: impl IntoIterator<Item = ComponentId>,
) -> HashSet<ComponentId> {
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
        .collect()
}

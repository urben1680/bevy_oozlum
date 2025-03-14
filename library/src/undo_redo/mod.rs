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
        archetype::ArchetypeId,
        bundle::BundleId,
        component::{Component, ComponentCloneBehavior, ComponentId},
        entity::{hash_set::EntityHashSet, Entity, EntityCloner},
        hierarchy::Children,
        resource::Resource,
        system::{Commands, EntityCommands},
        world::{DeferredWorld, EntityWorldMut, FromWorld, World},
    },
    platform_support::{
        collections::{HashMap, HashSet},
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
            .expect(EXPECT_BUFFER)
            .buffer_undo_redo(undo_redo);
        self
    }
}

impl BuffersUndoRedo for DeferredWorld<'_> {
    fn buffer_undo_redo(&mut self, undo_redo: impl UndoRedo) -> &mut Self {
        self.get_resource_mut::<UndoRedoBuffer>()
            .expect(EXPECT_BUFFER)
            .buffer_undo_redo(undo_redo);
        self
    }
}

impl BuffersUndoRedo for UndoRedoBuffer {
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
pub struct UndoRedoBuffer {
    undo_redo_buffer: VecDeque<SyncCell<Box<dyn UndoRedo>>>,
}

impl UndoRedoBuffer {
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

pub trait RevWorld {
    fn rev_despawn(&mut self, entity: Entity) -> bool;
    fn buffer_components(
        &mut self,
        entity: Entity,
        at: BufferAt,
        components: Vec<ComponentId>,
    ) -> &mut Self;
    fn buffer_components_cached(
        &mut self,
        entity: Entity,
        key: impl Hash,
        components: impl FnOnce(&mut World) -> (BufferAt, Vec<ComponentId>),
    ) -> &mut Self;
    fn buffer_components_in_progress(&self) -> bool;
}

impl RevWorld for World {
    fn rev_despawn(&mut self, entity: Entity) -> bool {
        let entity_mut = self.entity_mut(entity);
        rev_despawn_inner(entity_mut)
    }

    fn buffer_components(
        &mut self,
        entity: Entity,
        at: BufferAt,
        components: Vec<ComponentId>,
    ) -> &mut Self {
        if components.is_empty() {
            return self;
        }
        let bundle = components_to_bundle(self, components);
        buffer_bundle(self, entity, at, bundle);
        self
    }

    fn buffer_components_cached(
        &mut self,
        entity: Entity,
        key: impl Hash,
        components: impl FnOnce(&mut World) -> (BufferAt, Vec<ComponentId>),
    ) -> &mut Self {
        #[derive(Resource, Default)]
        pub(crate) struct CachedBundles(HashMap<u64, Option<(BufferAt, BundleId)>, PassHash>);

        let mut hasher = FixedHasher::default().build_hasher();
        key.hash(&mut hasher);
        let key = hasher.finish();

        let mut cache = self.remove_resource::<CachedBundles>().unwrap_or_default();

        let maybe_components = *cache.0.entry(key).or_insert_with(|| {
            let (at, components) = components(self);
            if components.is_empty() {
                None
            } else {
                Some((at, components_to_bundle(self, components)))
            }
        });

        self.insert_resource(cache);

        if let Some((at, bundle)) = maybe_components {
            buffer_bundle(self, entity, at, bundle);
        }

        self
    }

    fn buffer_components_in_progress(&self) -> bool {
        self.contains_resource::<BufferBundleInProgress>()
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
            let component_info = world.components().get_info(component_id).expect("todo");
            if component_info.clone_behavior() == &ComponentCloneBehavior::Ignore {
                bevy::log::error!(
                    "Component {} is unclonable, it's insert, remove or overwrite will \
                    not be reversible, see https://github.com/bevyengine/bevy/issues/18079",
                    component_info.name()
                );
            }
        }
    }
    world.insert_resource(checked);

    world.register_dynamic_bundle(&components).id()
}

fn buffer_bundle(world: &mut World, entity: Entity, at: BufferAt, bundle: BundleId) {
    let mut buffer = BundleBuffer::new(world, entity, bundle);
    match at {
        BufferAt::Now => {
            buffer.move_bundle(world);
            world.buffer_undo_redo(buffer);
        }
        BufferAt::Undo => {
            world.buffer_undo_redo(buffer);
        }
        BufferAt::NowAndUndo => {
            let at_undo = buffer.clone(); // no buffer entity set yet so each spawns their own
            buffer.move_bundle(world);
            world.buffer_undo_redo(buffer).buffer_undo_redo(at_undo);
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

impl BundleBuffer {
    fn new(world: &World, entity: Entity, bundle: BundleId) -> Self {
        let meta = world.get_resource::<RevMeta>().expect("todo");
        let marker = DespawnAtOutOfLog::new(meta);
        Self {
            bundle,
            entity,
            state: BufferState::Unspawned(marker),
        }
    }
    fn move_bundle(&mut self, world: &mut World) {
        let (target, source);
        self.state = match self.state {
            BufferState::Unspawned(marker) => {
                target = world.spawn(marker).id();
                source = self.entity;
                BufferState::Filled(target)
            }
            BufferState::Empty(buffer) => {
                target = buffer;
                source = self.entity;
                BufferState::Filled(buffer)
            }
            BufferState::Filled(buffer) => {
                target = self.entity;
                source = buffer;
                BufferState::Empty(buffer)
            }
        };

        let bundle = world.bundles().get(self.bundle).expect("todo");
        let components: Box<[ComponentId]> = bundle.explicit_components().into();

        let progress_res = world.buffer_components_in_progress();
        if !progress_res {
            world.insert_resource(BufferBundleInProgress);
        }
        EntityCloner::build(world)
            .deny_all()
            .move_components(true)
            .without_required_components(|builder| {
                builder.allow_by_ids(components.iter().copied());
            })
            .clone_entity(source, target);
        if !progress_res {
            world.remove_resource::<BufferBundleInProgress>();
        }
    }
}

impl UndoRedo for BundleBuffer {
    fn undo(&mut self, world: &mut World) {
        self.move_bundle(world);
    }
    fn redo(&mut self, world: &mut World) {
        self.move_bundle(world);
    }
}

#[derive(Resource)]
struct BufferBundleInProgress;

// todo: remove this when linked issue was fixed
fn check_unclonable_components(world: &mut World, ids: &[ComponentId]) {
    #[derive(Resource, Default)]
    struct CheckedClonableIds(HashSet<ComponentId>);
    let mut checked = world
        .remove_resource::<CheckedClonableIds>()
        .unwrap_or_default();
    for &component_id in ids {
        if checked.0.insert(component_id) {
            let component_info = world.components().get_info(component_id).expect("todo");
            if component_info.clone_behavior() == &ComponentCloneBehavior::Ignore {
                bevy::log::error!(
                    "Component {} is unclonable, it's insert, remove or overwrite will not \
                    be reversible, see https://github.com/bevyengine/bevy/issues/18079",
                    component_info.name()
                );
            }
        }
    }
    world.insert_resource(checked);
}

#[derive(Component, Clone, Copy, Debug, Hash, PartialOrd, Ord, PartialEq, Eq)]
#[component(immutable)]
pub struct DespawnAtOutOfLog(u64);

impl DespawnAtOutOfLog {
    pub fn new(meta: &RevMeta) -> Self {
        assert_eq!(meta.get_direction(), Some(RevDirection::NOT_LOG));
        Self(meta.now())
    }
    pub fn added_at(self) -> u64 {
        self.0
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
                    .get_resource_mut::<UndoRedoBuffer>()
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
    ($($hashable: expr),*) => {
        // extra scope to keep `UniquePerInvoke`s isolated
        {
            #[derive(Copy, Clone)]
            struct UniquePerInvoke;
            (std::any::TypeId::of::<UniquePerInvoke>(), $($hashable,)*)
        }
    }
}

pub use unique_for_location;

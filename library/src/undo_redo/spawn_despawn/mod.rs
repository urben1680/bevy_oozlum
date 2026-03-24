use std::sync::Arc;

use bevy_ecs::{
    bundle::Bundle,
    change_detection::MaybeLocation,
    component::Component,
    entity::{Entity, EntityHashSet},
    resource::Resource,
    world::{
        EntityMut, EntityMutExcept, EntityRef, EntityRefExcept, EntityWorldMut, FilteredEntityMut,
        FilteredEntityRef, FromWorld, World, error::EntityMutableFetchError,
    },
};
use bevy_log::error;

use crate::{
    log::{OutOfLog, TransitionsLog},
    meta::{NotLog, RevDirection, RevMeta},
    prelude::UndoRedo,
    undo_redo::{LOCATION_PREFIX, RevWorld, add_children, undo_redo_str},
};

#[cfg(test)]
mod test;

#[derive(Component, Debug, Clone, Copy)]
#[cfg_attr(feature = "bevy_reflect", derive(bevy_reflect::Reflect))]
#[component(immutable)]
/// Marker component for entities that were marked as despawned but the actual despawn is delayed
/// so this can be reversed. This component disables the Entity,
/// see [`World::register_disabling_component`]. For various entity pointer types you can use
/// [`IsRevDespawned::is_rev_despawned`] to check for the existence of this component. Such entities
/// should be treated as despawned in reversible logic.
///
/// # Reversible spawn
///
/// Entities that are spawned with [`RevWorld::rev_spawn`] or marked with
/// [`RevEntityWorldMut::rev_mark_spawned`] (or variants) will ...
///
/// - ... receive this component when the above actions are **undone** and should be treated as
/// despawned.
/// - ... have this component removed when the above actions are **redone** and should be treated as
/// spawned.
/// - ... will be despawned if the above actions are not **redone** before the next update with
/// [`RevDirection::NotLog`] runs.
///
/// # Reversible despawn
///
/// Entities that are despawned with [`RevWorld::rev_despawn`] or [`RevEntityWorldMut::rev_despawn`]
/// (or variants) will ...
///
/// - ... receive this component **immediately** and should be treated as despawned.
/// - ... have this component removed when the above actions are **undone** and should be treated as
/// spawned.
/// - ... receive this component when the above actions are **redone** and should be treated as
/// despawned.
/// - ... will be despawned if the above actions are not **undone** before the next update with
/// [`RevDirection::NotLog`] runs and the frame the reversible despawn happened falls behind
/// [`RevMeta::past_end`].
///
/// # Notes
///
/// - The APIs mind linked entities based on [`RelationshipTarget::LINKED_SPAWN`].
/// - Manually inserting or removing this component is discouraged because no finalized despawn will
/// take place in these cases.
///
/// [`RevWorld::rev_spawn`]: crate::undo_redo::RevWorld::rev_spawn
/// [`RevEntityWorldMut::rev_mark_spawned`]: crate::undo_redo::RevEntityWorldMut::rev_mark_spawned
/// [`RevWorld::rev_despawn`]: crate::undo_redo::RevWorld::rev_despawn
/// [`RevEntityWorldMut::rev_despawn`]: crate::undo_redo::RevEntityWorldMut::rev_despawn
/// [`RelationshipTarget::LINKED_SPAWN`]: bevy_ecs::relationship::RelationshipTarget::LINKED_SPAWN

// todo: store MaybeLocation in component change meta instead of here, https://github.com/bevyengine/bevy/issues/20494
pub struct RevDespawned(pub MaybeLocation);

/// Despawn entities that are currently considered reversibly despawned and their relevant operation
/// to revert that fell out of log.
///
/// # Errors
///
/// - If [`RevMeta`] is not in the `world`, this will return [`DespawnFinalizerErr::MetaMissing`].
/// - If `RevMeta` is not currently [running], this will return
///   [`DespawnFinalizerErr::MetaNotRunning`].
/// - If the internal log went out of log, this will return [`DespawnFinalizerErr::OutOfLog`]. This
///   can happen if this is called more than once while [`RevUpdate`] ran.
///
/// [running]: RevMeta::running_direction
/// [`RevUpdate`]: crate::schedule::RevUpdate
pub fn finalize_despawns(world: &mut World) -> Result<(), DespawnFinalizerErr> {
    world
        .try_resource_scope::<DespawnFinalizer, _>(|world, this| {
            let this = this.into_inner();
            let meta = world
                .get_resource::<RevMeta>()
                .ok_or(DespawnFinalizerErr::MetaMissing)?;
            let direction = meta
                .get_running_direction()
                .ok_or(DespawnFinalizerErr::MetaNotRunning)?;

            match direction {
                RevDirection::NotLog(not_log) => {
                    let spawn = this.spawn_queue.drain(..).map(|(entity, _)| entity);
                    let despawn = this.despawn_queue.drain(..).map(|(entity, _)| entity);
                    let spawn_amount = spawn.len();
                    let mut drain = this.spawn_despawn.forward_extend_with(
                        meta,
                        not_log,
                        spawn.chain(despawn),
                        spawn_amount,
                    );

                    let mut past_drain = drain.past();
                    while let Some((entities, spawn_amount)) = past_drain.next_log_entry() {
                        for entity in entities.skip(spawn_amount) {
                            let _ = world.try_despawn(entity);
                        }
                    }
                    drop(past_drain);

                    let mut future_drain = drain.future();
                    while let Some((entities, spawn_amount)) = future_drain.next_log_entry() {
                        for entity in entities.take(spawn_amount) {
                            let _ = world.try_despawn(entity);
                        }
                    }
                }
                RevDirection::ForwardLog => {
                    if this.init_at <= meta.now() {
                        this.spawn_despawn.forward_log(meta)?;
                    }
                }
                RevDirection::BackwardLog => {
                    if this.init_at <= meta.now() {
                        this.spawn_despawn.backward_log(meta)?;
                    }
                }
            }
            Ok(())
        })
        .unwrap_or(Ok(()))
}

/// Error type that [`finalize_despawns`] may return.
#[derive(Debug)]
pub enum DespawnFinalizerErr {
    /// [`RevMeta`] is not in the world.
    MetaMissing,

    /// [`RevMeta`] is not currently [running]
    ///
    /// [running]: RevMeta::running_direction
    MetaNotRunning,

    /// The internal log went out of log. This can happen if `finalize_despawns` is called more than
    /// once while [`RevUpdate`] ran.
    ///
    /// [`RevUpdate`]: crate::schedule::RevUpdate
    OutOfLog,
}

impl From<OutOfLog> for DespawnFinalizerErr {
    fn from(_: OutOfLog) -> Self {
        Self::OutOfLog
    }
}

/// Mark multiple entities and their children as spawned/despawned.
pub(super) fn mark_entities<const SPAWN: bool>(
    not_log: NotLog,
    world: &mut World,
    entities: &[Entity],
    include_unlinked_related: bool,
    caller: MaybeLocation,
) {
    let mut entities_set = EntityHashSet::with_capacity(entities.len());
    for &entity in entities {
        if !entities_set.insert(entity) {
            continue;
        }
        let Ok(entity_ref) = world.get_entity(entity) else {
            entities_set.remove(&entity);
            continue;
        };
        if entity_ref.is_rev_despawned() {
            entities_set.remove(&entity);
            continue;
        }
        add_children(
            world,
            entity_ref,
            &mut entities_set,
            include_unlinked_related,
        );
    }

    if !entities_set.is_empty() {
        mark_inner::<SPAWN>(not_log, world, entities_set, caller);
    }
}

/// Mark a single entity and its children as spawned/despawned.
pub(super) fn mark_entity<const SPAWN: bool>(
    not_log: NotLog,
    entity: &mut EntityWorldMut,
    include_unlinked_related: bool,
    caller: MaybeLocation,
) -> bool {
    if entity.is_rev_despawned() {
        return false;
    }

    let mut entities_set = EntityHashSet::from([entity.id()]);

    add_children(
        entity.world(),
        (&*entity).into(),
        &mut entities_set,
        include_unlinked_related,
    );

    entity.world_scope(|world| mark_inner::<SPAWN>(not_log, world, entities_set, caller));

    true
}

/// Mark a single empty entity as spawned.
pub(super) fn mark_spawn_empty(
    not_log: NotLog,
    entity: &mut EntityWorldMut,
    caller: MaybeLocation,
) {
    let id = entity.id();
    let spawn_despawn = RevSpawnDespawn::<_, true> {
        entities: id,
        caller,
    };

    entity.world_scope(|world| {
        world
            .get_resource_or_init::<DespawnFinalizer>()
            .spawn_queue
            .push((id, caller));
        world.buffer_undo_redo(not_log, spawn_despawn, caller);
    })
}

/// Mark multiple entities as spawned/despawned.
fn mark_inner<const SPAWN: bool>(
    not_log: NotLog,
    world: &mut World,
    entities: EntityHashSet,
    caller: MaybeLocation,
) {
    let spawn_despawn = RevSpawnDespawn::<_, SPAWN> { entities, caller };

    let iter = spawn_despawn
        .entities
        .iter()
        .map(|&entity| (entity, caller));

    let mut resource = world.get_resource_or_init::<DespawnFinalizer>();

    if SPAWN {
        resource.spawn_queue.extend(iter);
        world.buffer_undo_redo(not_log, spawn_despawn, caller);
    } else {
        resource.despawn_queue.extend(iter);
        world.redo_and_buffer(not_log, spawn_despawn, caller);
    }
}

struct RevSpawnDespawn<E, const SPAWN: bool> {
    pub(super) entities: E,
    pub(super) caller: MaybeLocation,
}

impl<E: EntityCollection, const SPAWN: bool> RevSpawnDespawn<E, SPAWN> {
    fn undo_redo<const UNDO: bool>(&self, world: &mut World) {
        let spawn_despawn = if SPAWN { "spawn" } else { "despawn" };
        if SPAWN ^ UNDO {
            // redo spawn / undo despawn
            for entity in self.entities.iter_entities() {
                match world.get_entity_mut(entity) {
                    Ok(mut entity_mut) => {
                        entity_mut.remove::<RevDespawned>();
                    }
                    Err(EntityMutableFetchError::NotSpawned(err)) => error!(
                        "{} of reversible {spawn_despawn}{LOCATION_PREFIX}{} failed: {err}",
                        undo_redo_str::<UNDO>(),
                        self.caller
                    ),
                    Err(EntityMutableFetchError::AliasedMutability(_)) => unreachable!(),
                }
            }
        } else {
            // undo spawn / redo despawn
            if let Err(err) = world.try_insert_batch(
                self.entities
                    .iter_entities()
                    .map(|entity| (entity, RevDespawned(self.caller))),
            ) {
                error!(
                    "{} of reversible {spawn_despawn}{LOCATION_PREFIX}{} (partially) failed: {err}",
                    undo_redo_str::<UNDO>(),
                    self.caller
                );
            }
        }
    }
}

impl<E: EntityCollection, const SPAWN: bool> UndoRedo for RevSpawnDespawn<E, SPAWN> {
    fn undo(&mut self, world: &mut World) {
        self.undo_redo::<true>(world);
    }
    fn redo(&mut self, world: &mut World) {
        self.undo_redo::<false>(world);
    }
}

pub(super) trait EntityCollection: Send + Sync + 'static {
    fn iter_entities(&self) -> impl Iterator<Item = Entity>;
}

impl EntityCollection for EntityHashSet {
    fn iter_entities(&self) -> impl Iterator<Item = Entity> {
        self.iter().copied()
    }
}

impl EntityCollection for Arc<[Entity]> {
    fn iter_entities(&self) -> impl Iterator<Item = Entity> {
        self.iter().copied()
    }
}

impl EntityCollection for Entity {
    fn iter_entities(&self) -> impl Iterator<Item = Entity> {
        [*self].into_iter()
    }
}

/// Tracks which entities got reversibly spawned or despawned to finalize the despawn if applicable.
#[derive(Resource, Debug)]
struct DespawnFinalizer {
    spawn_despawn: TransitionsLog<Entity, usize>,
    spawn_queue: Vec<(Entity, MaybeLocation)>,
    despawn_queue: Vec<(Entity, MaybeLocation)>,
    init_at: u64,
}

impl FromWorld for DespawnFinalizer {
    fn from_world(world: &mut World) -> Self {
        let init_at = world
            .get_resource::<RevMeta>()
            .filter(|meta| {
                meta.get_running_direction()
                    .is_some_and(RevDirection::is_not_log)
            })
            .map(|meta| meta.now())
            .unwrap_or_else(|| {
                error!(
                    "a reversible spawn, despawn or marking an entity as such was attempted \
                    outside RevDirection::NotLog, this may cause an out-of-log error when \
                    attempting to undo this, do not store NotLog to do reversible operations"
                );
                1 // 0 would be an invalid value for RevDirection::NotLog regardless of this error
            });
        Self {
            spawn_despawn: Default::default(),
            spawn_queue: Default::default(),
            despawn_queue: Default::default(),
            init_at,
        }
    }
}

/// Extension trait for entity pointers to check if an entity is reversibly despawned.
///
/// See the [`RevDespawned`](super::RevDespawned) documentation to understand the mechanics of
/// reversible spawn/despawn.
pub trait IsRevDespawned {
    /// Returns `true` if the entity is markes as reversibly despawned. In that case the entity
    /// should be treated as despawned. Returns `false` otherwise.
    ///
    /// See the [`RevDespawned`](super::RevDespawned) documentation to understand the mechanics of
    /// reversible spawn/despawn.
    fn is_rev_despawned(&self) -> bool;
}

impl IsRevDespawned for EntityRef<'_> {
    fn is_rev_despawned(&self) -> bool {
        self.contains::<RevDespawned>()
    }
}

impl<B: Bundle> IsRevDespawned for EntityRefExcept<'_, '_, B> {
    fn is_rev_despawned(&self) -> bool {
        self.contains::<RevDespawned>()
    }
}

impl IsRevDespawned for FilteredEntityRef<'_, '_> {
    fn is_rev_despawned(&self) -> bool {
        self.contains::<RevDespawned>()
    }
}

impl IsRevDespawned for EntityMut<'_> {
    fn is_rev_despawned(&self) -> bool {
        self.contains::<RevDespawned>()
    }
}

impl<B: Bundle> IsRevDespawned for EntityMutExcept<'_, '_, B> {
    fn is_rev_despawned(&self) -> bool {
        self.contains::<RevDespawned>()
    }
}

impl IsRevDespawned for FilteredEntityMut<'_, '_> {
    fn is_rev_despawned(&self) -> bool {
        self.contains::<RevDespawned>()
    }
}

impl IsRevDespawned for EntityWorldMut<'_> {
    fn is_rev_despawned(&self) -> bool {
        self.contains::<RevDespawned>()
    }
}

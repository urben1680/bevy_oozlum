use std::{any::TypeId, hash::Hash, marker::PhantomData};

use bevy::{
    ecs::{
        archetype::ArchetypeId,
        bundle::{Bundle, BundleFromComponents, BundleId, InsertMode, NoBundleEffect},
        change_detection::MaybeLocation,
        component::{Component, ComponentId},
        entity::{Entity, EntityCloner, EntityClonerBuilder, OptIn, OptOut},
        resource::Resource,
        world::{EntityRef, EntityWorldMut, World},
    },
    platform::collections::{HashMap, HashSet}, reflect::Type,
};

use crate::{
    meta::{NonLogNow, RevDirection},
    undo_redo::{
        EntityRevDespawnedError, RevDespawned, RevOpInProgress
    },
};

use super::{
    BuffersUndoRedo, RevDespawnCleaner, RevWorld, Spawn, Take, UndoRedo, UndoRedoSwap,
    rev_spawn_finish,
};

// wip, consider observer approach for to-buffer-move and linked despawn
//pub mod relationship;
/*
on register:

*/

#[cfg(test)]
mod test;

pub trait RevEntityWorldMut<'w> {
    fn redo_and_buffer(&mut self, now: NonLogNow, undo_redo: impl UndoRedo);

    // the methods here are purposely sorted alphabetically to make it easily comparable to bevy's docs
    // unmentioned methods are either
    // a) unrelated to reversible structural changes OR
    // b) deprecated in bevy OR
    // c) missed by accident!

    /// Reversible version of [`EntityWorldMut::clear`].
    fn rev_clear(&mut self, now: NonLogNow) -> &mut Self;

    /// Reversible version of [`EntityWorldMut::clone_and_spawn`].
    ///
    /// Note that if `self` is in relationship with another entity, these relationship types need to be
    /// registered with [`RevApp::register_non_entity_buffer`](crate::app::RevApp::register_non_entity_buffer).
    /// Otherwise, at undo, the spawned entity will still be in relationship with the common
    /// `RelationshipTarget` despite being [temporarily despawned](DisabledToDespawn).
    fn rev_clone_and_spawn(&mut self, now: NonLogNow) -> Entity;

    /// Reversible version of [`EntityWorldMut::clone_and_spawn_with_opt_in`].
    ///
    /// Note that if `self` is in relationship with another entity, these relationship types need to be
    /// registered with [`RevApp::register_non_entity_buffer`](crate::app::RevApp::register_non_entity_buffer).
    /// Otherwise, at undo, the spawned entity will still be in relationship with the common
    /// `RelationshipTarget` despite being [temporarily despawned](DisabledToDespawn).
    fn rev_clone_and_spawn_with_opt_in(
        &mut self,
        now: NonLogNow,
        config: impl FnOnce(&mut EntityClonerBuilder<OptIn>) + Send + Sync + 'static,
    ) -> Entity;

    /// Reversible version of [`EntityWorldMut::clone_and_spawn_with_opt_out].
    ///
    /// Note that if `self` is in relationship with another entity, these relationship types need to be
    /// registered with [`RevApp::register_non_entity_buffer`](crate::app::RevApp::register_non_entity_buffer).
    /// Otherwise, at undo, the spawned entity will still be in relationship with the common
    /// `RelationshipTarget` despite being [temporarily despawned](DisabledToDespawn).
    fn rev_clone_and_spawn_with_opt_out(
        &mut self,
        now: NonLogNow,
        config: impl FnOnce(&mut EntityClonerBuilder<OptOut>) + Send + Sync + 'static,
    ) -> Entity;

    // rev_clone_components
    // out of scope

    // rev_clone_with
    // out of scope due complexity

    /// Reversible version of [`EntityWorldMut::despawn`].
    ///
    /// Note that this despawns the entity not now but later when this action goes out of log.
    fn rev_despawn_single(self, now: NonLogNow);

    /// Reversible version of [`EntityWorldMut::entry`].
    fn rev_entry<'a, T: Component>(&'a mut self) -> RevComponentEntry<'w, 'a, T>;

    /// Reversible version of [`EntityWorldMut::insert`].
    fn rev_insert<T: Bundle>(&mut self, now: NonLogNow, bundle: T) -> &mut Self;

    // rev_insert_by_id
    // out of scope

    // rev_insert_by_ids
    // out of scope

    /// Reversible version of [`EntityWorldMut::insert_if_new`].
    fn rev_insert_if_new<T: Bundle>(&mut self, now: NonLogNow, bundle: T) -> &mut Self;

    // rev_insert_reflect
    // out of scope due complexity

    // rev_insert_reflect_with_registry
    // out of scope due complexity

    // rev_insert_with_relationship_hook_mode
    // missing EntityCloner API with RelationshipHookMode

    // rev_move_components
    // out of scope

    /// Reversible version of [`EntityWorldMut::remove`].
    fn rev_remove<T: Bundle>(&mut self, now: NonLogNow) -> &mut Self;

    // rev_remove_by_id
    // out of scope

    // rev_remove_by_ids
    // out of scope

    // rev_remove_reflect
    // out of scope due complexity

    // rev_remove_reflect_with_registry
    // out of scope due complexity

    /// Reversible version of [`EntityWorldMut::retain`].
    fn rev_retain<T: Bundle>(&mut self, now: NonLogNow) -> &mut Self;

    /// Reversible version of [`EntityWorldMut::take`].
    fn rev_take<'a, T: Bundle + BundleFromComponents, Out>(
        &'a mut self,
        now: NonLogNow,
        c: impl FnOnce(&T) -> Out,
    ) -> Option<Out>;
}

impl<'w> RevEntityWorldMut<'w> for EntityWorldMut<'w> {
    fn redo_and_buffer(&mut self, now: NonLogNow, undo_redo: impl UndoRedo) {
        self.world_scope(|world| world.redo_and_buffer(now, undo_redo))
    }

    #[track_caller]
    fn rev_clear(&mut self, now: NonLogNow) -> &mut Self {
        rev_try_clear_with_caller(self, now, MaybeLocation::caller()).unwrap();
        self
    }

    #[track_caller]
    fn rev_clone_and_spawn(&mut self, now: NonLogNow) -> Entity {
        self.rev_clone_and_spawn_with_opt_out(now, |_| {})
    }

    #[track_caller]
    fn rev_clone_and_spawn_with_opt_in(
        &mut self,
        now: NonLogNow,
        config: impl FnOnce(&mut EntityClonerBuilder<OptIn>) + Send + Sync + 'static,
    ) -> Entity {
        rev_try_clone_and_spawn_with_opt_in_with_caller(self, now, config, MaybeLocation::caller())
            .unwrap()
    }

    #[track_caller]
    fn rev_clone_and_spawn_with_opt_out(
        &mut self,
        now: NonLogNow,
        config: impl FnOnce(&mut EntityClonerBuilder<OptOut>) + Send + Sync + 'static,
    ) -> Entity {
        rev_try_clone_and_spawn_with_opt_out_with_caller(self, now, config, MaybeLocation::caller())
            .unwrap()
    }

    #[track_caller]
    fn rev_despawn_single(self, now: NonLogNow) {
        rev_try_despawn_single_with_caller(self, now, MaybeLocation::caller()).unwrap()
    }

    fn rev_entry<'a, T: Component>(&'a mut self) -> RevComponentEntry<'w, 'a, T> {
        if self.contains::<T>() {
            RevComponentEntry::Occupied(RevOccupiedComponentEntry {
                entity_world_mut: self,
                _marker: PhantomData,
            })
        } else {
            RevComponentEntry::Vacant(RevVacantComponentEntry {
                entity_world_mut: self,
                _marker: PhantomData,
            })
        }
    }

    #[track_caller]
    fn rev_insert<T: Bundle>(&mut self, now: NonLogNow, bundle: T) -> &mut Self {
        rev_try_insert_with_caller(
            self,
            bundle,
            InsertMode::Replace,
            now,
            MaybeLocation::caller(),
        )
        .unwrap()
    }

    #[track_caller]
    fn rev_insert_if_new<T: Bundle>(&mut self, now: NonLogNow, bundle: T) -> &mut Self {
        rev_try_insert_with_caller(self, bundle, InsertMode::Keep, now, MaybeLocation::caller())
            .unwrap()
    }

    #[track_caller]
    fn rev_remove<T: Bundle>(&mut self, now: NonLogNow) -> &mut Self {
        rev_try_remove_with_caller::<T, false>(self, now, MaybeLocation::caller()).unwrap()
    }

    #[track_caller]
    fn rev_retain<T: Bundle>(&mut self, now: NonLogNow) -> &mut Self {
        rev_try_retain_with_caller::<T>(self, now, MaybeLocation::caller()).unwrap()
    }

    #[track_caller]
    fn rev_take<'a, T: Bundle + BundleFromComponents, Out>(
        &'a mut self,
        now: NonLogNow,
        c: impl FnOnce(&T) -> Out,
    ) -> Option<Out> {
        self.take::<T>().map(|value| {
            let out = c(&value);
            let entity = self.id();
            self.buffer_undo_redo(
                now,
                Take {
                    bundle: Some(value),
                    entity,
                },
            );
            out
        })
    }
}

pub(crate) fn rev_try_clear_with_caller<'a, 'b>(
    entity_mut: &'a mut EntityWorldMut<'b>,
    now: NonLogNow,
    caller: MaybeLocation,
) -> Result<&'a mut EntityWorldMut<'b>, EntityRevDespawnedError> {
    assert_not_rev_despawned(&*entity_mut)?;
    let id = entity_mut.id();
    entity_mut.world_scope(|world| {
        let mut buffer = BundleBuffer::new((), id, caller);
        let mut cloner = ().cloner(world);
        let entities = buffer.toggle_state(world);
        entities.move_components(world, &mut cloner, RevDirection::NOT_LOG);
        world.buffer_undo_redo(now, buffer);
    });
    Ok(entity_mut)
}

pub(crate) fn rev_try_clone_and_spawn_with_opt_in_with_caller<'a, 'b>(
    entity_mut: &'a mut EntityWorldMut<'b>,
    now: NonLogNow,
    config: impl FnOnce(&mut EntityClonerBuilder<OptIn>) + Send + Sync + 'static,
    caller: MaybeLocation,
) -> Result<Entity, EntityRevDespawnedError> {
    assert_not_rev_despawned(&*entity_mut)?;
    let clone = entity_mut.clone_and_spawn_with_opt_in(config);
    rev_spawn_finish(entity_mut, now, clone, caller);
    Ok(clone)
}

pub(crate) fn rev_try_clone_and_spawn_with_opt_out_with_caller<'a, 'b>(
    entity_mut: &'a mut EntityWorldMut<'b>,
    now: NonLogNow,
    config: impl FnOnce(&mut EntityClonerBuilder<OptOut>) + Send + Sync + 'static,
    caller: MaybeLocation,
) -> Result<Entity, EntityRevDespawnedError> {
    assert_not_rev_despawned(&*entity_mut)?;
    let clone = entity_mut.clone_and_spawn_with_opt_out(config);
    rev_spawn_finish(entity_mut, now, clone, caller);
    Ok(clone)
}

pub(crate) fn rev_try_despawn_single_with_caller(
    mut entity_mut: EntityWorldMut,
    now: NonLogNow,
    caller: MaybeLocation,
) -> Result<(), EntityRevDespawnedError> {
    assert_not_rev_despawned(&entity_mut)?;
    let entity = entity_mut.id();
    entity_mut.redo_and_buffer(
        now,
        UndoRedoSwap(Spawn {
            entity,
            location: caller,
        }),
    );
    entity_mut
        .resource_mut::<RevDespawnCleaner>()
        .log_despawn(entity, caller, now);
    Ok(())
}

pub(crate) fn rev_try_insert_with_caller<'a, 'b, T: Bundle>(
    entity_mut: &'a mut EntityWorldMut<'b>,
    bundle: T,
    mut insert_mode: InsertMode,
    now: NonLogNow,
    caller: MaybeLocation,
) -> Result<&'a mut EntityWorldMut<'b>, EntityRevDespawnedError> {
    let archetype = assert_not_rev_despawned(&*entity_mut)?;
    let id = entity_mut.id();
    entity_mut.world_scope(|world| {
        // todo: manually do the logic of world_scope without a closure so #[track_caller] enters below logic
        let bundle_id =
            world.resource_scope::<BundleIdOfOpCache, _>(|world, mut cache| match insert_mode {
                InsertMode::Replace => {
                    let (bundle_id, updated_insert_mode) = cache.get_insert::<T>(world, archetype);
                    insert_mode = updated_insert_mode; // when there is nothing to replace, simplify to `Keep`
                    bundle_id
                }
                InsertMode::Keep => cache.get_insert_if_new::<T>(world, archetype),
            });
        let cloner_builder = BundleIdCloner::<false>(bundle_id);
        let mut buffer = BundleBuffer::new(cloner_builder, id, caller);
        world
            .resource_mut::<RevDespawnCleaner>()
            .log_spawn_buffer(None, caller); // reserve log entry for buffer of inserted components
        match insert_mode {
            InsertMode::Replace => {
                let mut cloner = cloner_builder.cloner(world);
                // here the `buffer` is the buffer for the overwritten components...
                let entities = buffer.toggle_state(world);
                let backup_buffer = entities.buffer;
                entities.move_components(world, &mut cloner, RevDirection::NOT_LOG);
                // ...here `buffer` becomes the buffer for the inserted components
                buffer.state = BufferState::Unspawned(caller);
                let buffer = BundleBufferReplace {
                    backup_buffer,
                    insert_buffer: buffer,
                };
                world.buffer_undo_redo(now, buffer);
                world.entity_mut(id).insert(bundle); // todo: upstream a way to set the location
            }
            InsertMode::Keep => {
                world.buffer_undo_redo(now, buffer);
                world.entity_mut(id).insert_if_new(bundle); // todo: upstream a way to set the location
            }
        }
    });
    Ok(entity_mut)
}

pub(crate) fn rev_try_remove_with_caller<'a, 'b, T: Bundle, const WITH_REQUIRED: bool>(
    entity_mut: &'a mut EntityWorldMut<'b>,
    now: NonLogNow,
    caller: MaybeLocation,
) -> Result<&'a mut EntityWorldMut<'b>, EntityRevDespawnedError> {
    assert_not_rev_despawned(&*entity_mut)?;
    let id = entity_mut.id();
    entity_mut.world_scope(|world| {
        let bundle_id = world.register_bundle::<T>().id();
        let cloner_builder = BundleIdCloner::<WITH_REQUIRED>(bundle_id);
        let mut buffer = BundleBuffer::new(cloner_builder, id, caller);
        let mut cloner = cloner_builder.cloner(world);
        let entities = buffer.toggle_state(world);
        entities.move_components(world, &mut cloner, RevDirection::NOT_LOG);
        world.buffer_undo_redo(now, buffer);
    });
    Ok(entity_mut)
}

pub(crate) fn rev_try_retain_with_caller<'a, 'b, T: Bundle>(
    entity_mut: &'a mut EntityWorldMut<'b>,
    now: NonLogNow,
    caller: MaybeLocation,
) -> Result<&'a mut EntityWorldMut<'b>, EntityRevDespawnedError> {
    let archetype = assert_not_rev_despawned(&*entity_mut)?;
    let id = entity_mut.id();
    entity_mut.world_scope(|world| {
        let bundle_id = world.resource_scope::<BundleIdOfOpCache, _>(|world, mut cache| {
            cache.get_retain::<T>(world, archetype)
        });
        // using an opt-out cloner does not work because moving would not opt out of required but required_by components of bundle components
        let cloner_builder = BundleIdCloner::<false>(bundle_id);
        let mut buffer = BundleBuffer::new(cloner_builder, id, caller);
        let mut cloner = cloner_builder.cloner(world);
        let entities = buffer.toggle_state(world);
        entities.move_components(world, &mut cloner, RevDirection::NOT_LOG);
        world.buffer_undo_redo(now, buffer);
    });
    Ok(entity_mut)
}

trait MaybeDynamicBundle {
    fn get_key(&self, world: &mut World) -> BundleCacheKey;
    fn to_bundle_id(self, key: BundleCacheKey, world: &mut World) -> BundleId;
}

impl<T: Bundle<Effect: NoBundleEffect>> MaybeDynamicBundle for PhantomData<T> {
    fn get_key(&self, _world: &mut World) -> BundleCacheKey {
        BundleCacheKey::Typed(TypeId::of::<T>())
    }
    fn to_bundle_id(self, _key: BundleCacheKey, world: &mut World) -> BundleId {
        world.register_bundle::<T>().id()
    }
}

impl MaybeDynamicBundle for &[ComponentId] {
    fn get_key(&self, world: &mut World) -> BundleCacheKey {
        BundleCacheKey::Dynamic(world.register_dynamic_bundle(self).id())
    }
    fn to_bundle_id(self, key: BundleCacheKey, _world: &mut World) -> BundleId {
        match key {
            BundleCacheKey::Dynamic(bundle_id) => bundle_id,
            BundleCacheKey::Typed(_) => unreachable!() 
        }
    }
}

#[derive(Copy, Clone, Hash, PartialEq, Eq)]
enum BundleCacheKey {
    Typed(TypeId),
    Dynamic(BundleId)
}

#[derive(Resource, Default)]
pub(crate) struct BundleIdOfOpCache {
    insert: HashMap<(ArchetypeId, BundleCacheKey), (BundleId, InsertMode)>,
    insert_if_new: HashMap<(ArchetypeId, BundleCacheKey), BundleId>,
    retain: HashMap<(ArchetypeId, BundleCacheKey), BundleId>,
}

impl BundleIdOfOpCache {
    fn get_insert<T: Bundle>(
        &mut self,
        world: &mut World,
        archetype_id: ArchetypeId,
        bundle: impl MaybeDynamicBundle
    ) -> (BundleId, InsertMode) {
        let key = (archetype_id, bundle.get_key(world));
        *self.insert.entry(key).or_insert_with(|| {
            // Bundle explicit:  A(2), B(2), C(2)
            // Bundle required:                    D(2), E(2)

            // Entity before:    A(1), B(1),             E(1)
            // Entity after:     A(2), B(2), C(2), D(2), E(1)

            // Buffer 1:         A(1), B(1), C(*), D(*), E(_)
            // Buffer 2 at undo: A(2), B(2), C(2), D(2), E(_)

            // * = if any appear at redo, _ = unused default

            let bundle_id = bundle.to_bundle_id(key.1, world);
            let bundle = world.bundles().get(bundle_id).unwrap();
            let archetype = world.archetypes().get(archetype_id).unwrap();
            let components: Vec<_> = bundle
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
            let bundle = world.register_dynamic_bundle(&components).id();

            if overwrites {
                return (bundle, InsertMode::Replace);
            }

            self.insert_if_new.insert(key, bundle);
            (bundle, InsertMode::Keep)
        })
    }
    fn get_insert_if_new<T: Bundle>(
        &mut self,
        world: &mut World,
        archetype_id: ArchetypeId,
        bundle: impl MaybeDynamicBundle,
    ) -> BundleId {
        let key = (archetype_id, bundle.get_key(world));
        *self.insert_if_new.entry(key).or_insert_with(|| {
            // Bundle explicit:  A(2), B(2), C(2)
            // Bundle required:                    D(2), E(2)

            // Entity before:    A(1), B(1),             E(1)
            // Entity after:     A(1), B(1), C(2), D(2), E(1)

            // Buffer at undo:               C(2), D(2), E(_)

            // _ = unused default

            let bundle_id = bundle.to_bundle_id(key.1, world);
            let bundle = world.bundles().get(bundle_id).unwrap();
            let archetype = world.archetypes().get(archetype_id).unwrap();
            let components: Vec<_> = bundle
                .contributed_components()
                .iter()
                .copied()
                .filter(|component_id| !archetype.contains(*component_id))
                .collect();
            world.register_dynamic_bundle(&components).id()
        })
    }
    fn get_retain<T: Bundle>(&mut self, world: &mut World, archetype_id: ArchetypeId,
        bundle: impl MaybeDynamicBundle) -> BundleId {
        let key = (archetype_id, bundle.get_key(world));
        *self.retain.entry(key).or_insert_with(|| {
            let bundle_id = bundle.to_bundle_id(key.1, world);
            let bundle_components: HashSet<ComponentId> = world
                .bundles()
                .get(bundle_id)
                .unwrap()
                .contributed_components()
                .iter()
                .copied()
                .collect();
            let archetype = world.archetypes().get(archetype_id).unwrap();
            let components: Vec<_> = archetype
                .components()
                .filter(|component_id| !bundle_components.contains(component_id))
                .collect();
            world.register_dynamic_bundle(&components).id()
        })
    }
}

pub(crate) fn assert_not_rev_despawned<'a, E: 'a + Into<EntityRef<'a>>>(
    entity: E,
) -> Result<ArchetypeId, EntityRevDespawnedError> {
    let entity = entity.into();
    let id = entity.id();
    if entity.contains::<RevDespawned>() {
        return Err(EntityRevDespawnedError { entity: id });
    };
    Ok(entity.location().archetype_id)
}

struct BundleBuffer<Cloner> {
    cloner_builder: Cloner,
    entity: Entity,
    state: BufferState,
}

trait ClonerBuilder: Send + 'static {
    fn cloner(&self, world: &mut World) -> EntityCloner;
}

#[derive(Clone, Copy)]
struct BundleIdCloner<const WITH_REQUIRED: bool>(BundleId);

impl<const WITH_REQUIRED: bool> ClonerBuilder for BundleIdCloner<WITH_REQUIRED> {
    fn cloner(&self, world: &mut World) -> EntityCloner {
        let mut builder = EntityCloner::build_opt_in(world);
        builder
            .move_components(true)
            .without_required_components(|builder| {
                if WITH_REQUIRED {
                    builder.allow_by_ids(self.0);
                } else {
                    builder.allow_by_ids_if_new(self.0);
                }
            });
        builder.finish()
    }
}

impl ClonerBuilder for () {
    fn cloner(&self, world: &mut World) -> EntityCloner {
        let mut builder = EntityCloner::build_opt_out(world);
        builder.move_components(true);
        builder.finish()
    }
}

#[derive(Copy, Clone)]
enum BufferState {
    Unspawned(MaybeLocation),
    Empty(Entity),
    Filled(Entity),
}

struct BundleEntities {
    target: Entity,
    source: Entity,
    buffer: Entity,
}

impl BundleEntities {
    fn move_components(
        self,
        world: &mut World,
        cloner: &mut EntityCloner,
        direction: RevDirection,
    ) {
        let progress = RevOpInProgress::Buffer {
            direction,
            buffer: self.buffer,
        };
        progress.scope(world, |world| {
            cloner.clone_entity(world, self.source, self.target);
        })
    }
}

impl<Cloner: ClonerBuilder> BundleBuffer<Cloner> {
    fn new(cloner_builder: Cloner, entity: Entity, location: MaybeLocation) -> Self {
        Self {
            cloner_builder,
            entity,
            state: BufferState::Unspawned(location),
        }
    }
    fn toggle_state(&mut self, world: &mut World) -> BundleEntities {
        match self.state {
            BufferState::Unspawned(location) => {
                let buffer = world.spawn(RevDespawned).id();
                // todo: set location in RevDespawned change meta https://github.com/bevyengine/bevy/issues/20494
                world
                    .resource_mut::<RevDespawnCleaner>()
                    .log_spawn_buffer(Some(buffer), location);
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
    fn undo_redo(&mut self, world: &mut World, direction: RevDirection) {
        let mut cloner = self.cloner_builder.cloner(world);
        let entities = self.toggle_state(world);
        entities.move_components(world, &mut cloner, direction);
    }
}

impl<Cloner: ClonerBuilder> UndoRedo for BundleBuffer<Cloner> {
    fn undo(&mut self, world: &mut World) {
        self.undo_redo(world, RevDirection::BackwardLog);
    }
    fn redo(&mut self, world: &mut World) {
        self.undo_redo(world, RevDirection::FORWARD_LOG);
    }
}

struct BundleBufferReplace {
    backup_buffer: Entity,
    insert_buffer: BundleBuffer<BundleIdCloner<false>>,
}

impl UndoRedo for BundleBufferReplace {
    fn undo(&mut self, world: &mut World) {
        let mut cloner = self.insert_buffer.cloner_builder.cloner(world);

        // move inserted components from the entity into the insert_buffer
        let entities = self.insert_buffer.toggle_state(world);
        entities.move_components(world, &mut cloner, RevDirection::BackwardLog);

        // move backuped components from the backup_buffer into the entity
        let entities = BundleEntities {
            source: self.backup_buffer,
            target: self.insert_buffer.entity,
            buffer: self.backup_buffer,
        };
        entities.move_components(world, &mut cloner, RevDirection::BackwardLog);
    }
    fn redo(&mut self, world: &mut World) {
        let mut cloner = self.insert_buffer.cloner_builder.cloner(world);

        // move backuped components from the entity into the backup_buffer
        let entities = BundleEntities {
            source: self.insert_buffer.entity,
            target: self.backup_buffer,
            buffer: self.backup_buffer,
        };
        entities.move_components(world, &mut cloner, RevDirection::FORWARD_LOG);

        // move inserted components from the insert_buffer into the entity
        let entities = self.insert_buffer.toggle_state(world);
        entities.move_components(world, &mut cloner, RevDirection::FORWARD_LOG);
    }
}

// todo: upstream public EntityWorldMut getter for vanilla types
pub enum RevComponentEntry<'w, 'a, T: Component> {
    /// An occupied entry.
    Occupied(RevOccupiedComponentEntry<'w, 'a, T>),
    /// A vacant entry.
    Vacant(RevVacantComponentEntry<'w, 'a, T>),
}

pub struct RevOccupiedComponentEntry<'w, 'a, T> {
    entity_world_mut: &'a mut EntityWorldMut<'w>,
    _marker: PhantomData<T>,
}

pub struct RevVacantComponentEntry<'w, 'a, T> {
    entity_world_mut: &'a mut EntityWorldMut<'w>,
    _marker: PhantomData<T>,
}

impl<'w, 'a, T: Component> RevComponentEntry<'w, 'a, T> {
    /// See [`Entry::target_entity`](bevy::ecs::world::Entry::insert_entry).
    #[track_caller]
    pub fn insert_entry(self, component: T) -> RevOccupiedComponentEntry<'w, 'a, T> {
        match self {
            RevComponentEntry::Occupied(mut entry) => {
                entry.insert(component);
                entry
            }
            RevComponentEntry::Vacant(entry) => entry.insert(component),
        }
    }

    /// Reversible version of [`Entry::target_entity`](bevy::ecs::world::Entry::insert_entry).
    #[track_caller]
    pub fn rev_insert_entry(
        self,
        now: NonLogNow,
        component: T,
    ) -> RevOccupiedComponentEntry<'w, 'a, T> {
        match self {
            RevComponentEntry::Occupied(mut entry) => {
                entry.rev_insert(now, component);
                entry
            }
            RevComponentEntry::Vacant(entry) => entry.rev_insert(now, component),
        }
    }

    /// See [`Entry::or_insert`](bevy::ecs::world::Entry::or_insert).
    #[track_caller]
    pub fn or_insert(self, default: T) -> RevOccupiedComponentEntry<'w, 'a, T> {
        match self {
            RevComponentEntry::Occupied(entry) => entry,
            RevComponentEntry::Vacant(entry) => entry.insert(default),
        }
    }

    /// Reversible version of [`Entry::or_insert`](bevy::ecs::world::Entry::or_insert).
    #[track_caller]
    pub fn rev_or_insert(self, now: NonLogNow, default: T) -> RevOccupiedComponentEntry<'w, 'a, T> {
        match self {
            RevComponentEntry::Occupied(entry) => entry,
            RevComponentEntry::Vacant(entry) => entry.rev_insert(now, default),
        }
    }

    /// See [`Entry::or_insert_with`](bevy::ecs::world::Entry::or_insert_with).
    #[track_caller]
    pub fn or_insert_with<F: FnOnce() -> T>(
        self,
        default: F,
    ) -> RevOccupiedComponentEntry<'w, 'a, T> {
        match self {
            RevComponentEntry::Occupied(entry) => entry,
            RevComponentEntry::Vacant(entry) => entry.insert(default()),
        }
    }

    /// Reversible version of [`Entry::or_insert_with`](bevy::ecs::world::Entry::or_insert_with).
    #[track_caller]
    pub fn rev_or_insert_with<F: FnOnce() -> T>(
        self,
        now: NonLogNow,
        default: F,
    ) -> RevOccupiedComponentEntry<'w, 'a, T> {
        match self {
            RevComponentEntry::Occupied(entry) => entry,
            RevComponentEntry::Vacant(entry) => entry.rev_insert(now, default()),
        }
    }
}

impl<'w, 'a, T: Component + Default> RevComponentEntry<'w, 'a, T> {
    /// See [`Entry::or_insert_with`](bevy::ecs::world::Entry::or_default).
    #[track_caller]
    pub fn or_default(self) -> RevOccupiedComponentEntry<'w, 'a, T> {
        match self {
            RevComponentEntry::Occupied(entry) => entry,
            RevComponentEntry::Vacant(entry) => entry.insert(Default::default()),
        }
    }

    /// Reversible version of [`Entry::or_insert_with`](bevy::ecs::world::Entry::or_default).
    #[track_caller]
    pub fn rev_or_default(self, now: NonLogNow) -> RevOccupiedComponentEntry<'w, 'a, T> {
        match self {
            RevComponentEntry::Occupied(entry) => entry,
            RevComponentEntry::Vacant(entry) => entry.rev_insert(now, Default::default()),
        }
    }
}

impl<'w, 'a, T: Component> RevOccupiedComponentEntry<'w, 'a, T> {
    /// See [`OccupiedEntry::or_insert_with`](bevy::ecs::world::OccupiedEntry::insert).
    #[track_caller]
    pub fn insert(&mut self, component: T) {
        self.entity_world_mut.insert(component);
    }

    /// Reversible version of [`OccupiedEntry::or_insert_with`](bevy::ecs::world::OccupiedEntry::insert).
    #[track_caller]
    pub fn rev_insert(&mut self, now: NonLogNow, component: T) {
        self.entity_world_mut.rev_insert(now, component);
    }

    /// See [`OccupiedEntry::take`](bevy::ecs::world::OccupiedEntry::take).
    #[track_caller]
    pub fn take(self) -> T {
        // This shouldn't panic because if we have an OccupiedEntry the component must exist.
        self.entity_world_mut.take().unwrap()
    }

    /// Reversible version of [`OccupiedEntry::take`](bevy::ecs::world::OccupiedEntry::take).
    #[track_caller]
    pub fn rev_take<Out>(self, now: NonLogNow, c: impl FnOnce(&T) -> Out) -> Out {
        // This shouldn't panic because if we have an OccupiedEntry the component must exist.
        self.entity_world_mut.rev_take::<T, Out>(now, c).unwrap()
    }
}

impl<'w, 'a, T: Component> RevVacantComponentEntry<'w, 'a, T> {
    /// See [`VacantEntry::take`](bevy::ecs::world::VacantEntry::insert).
    #[track_caller]
    pub fn insert(self, component: T) -> RevOccupiedComponentEntry<'w, 'a, T> {
        self.entity_world_mut.insert(component);
        RevOccupiedComponentEntry {
            entity_world_mut: self.entity_world_mut,
            _marker: PhantomData,
        }
    }

    /// Reversible version of [`VacantEntry::take`](bevy::ecs::world::VacantEntry::insert).
    #[track_caller]
    pub fn rev_insert(self, now: NonLogNow, component: T) -> RevOccupiedComponentEntry<'w, 'a, T> {
        self.entity_world_mut.rev_insert(now, component);
        RevOccupiedComponentEntry {
            entity_world_mut: self.entity_world_mut,
            _marker: PhantomData,
        }
    }
}

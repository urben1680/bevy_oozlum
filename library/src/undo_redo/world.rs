use std::{error::Error, fmt::Display};

use super::{BuffersUndoRedo, UndoRedo};
use crate::{
    meta::{MetaPastLen, RevDirection, RevMeta},
    undo_redo::{
        EntityRevDespawnedError, RevBundle, RevEntityWorldMutInternal, RevInsertResourceNew,
        RevInsertResourceOverwrite, RevRemoveResource, mark_entities, mark_entity,
        mark_spawn_empty,
    },
};
use bevy_ecs::{
    bundle::{Bundle, NoBundleEffect},
    change_detection::MaybeLocation,
    component::ComponentId,
    entity::{Entity, EntityNotSpawnedError, SpawnError},
    resource::Resource,
    world::{EntityWorldMut, FromWorld, Mut, World, error::EntityMutableFetchError},
};
use bevy_utils::prelude::DebugName;

/// Extension trait for [`World`] with reversible variants of various methods.
pub trait RevWorld {
    /// Shorthand method of [`World::buffer_undo_redo`] with applying `undo_redo.redo(&mut self)`
    /// immediately. Useful when there is no difference between doing and redoing.
    fn redo_and_buffer(&mut self, meta_past_len: MetaPastLen, undo_redo: impl UndoRedo);

    /// Shorthand method of [`RevMeta::get_running_direction`].
    fn get_running_direction(&self) -> Option<RevDirection>;

    /// Helper method to mark an entity as reversibly spawned. Useful when the actual spawn is
    /// hidden and cannot be done with [`World::rev_spawn`].
    ///
    /// When possible, use `World::rev_spawn` instead.
    ///
    /// See the [`RevDespawned`](super::RevDespawned) documentation to understand the mechanics of
    /// reversible spawn/despawn.
    fn rev_mark_spawned(
        &mut self,
        meta_past_len: MetaPastLen,
        entity: Entity,
        include_unlinked_related: bool,
    ) -> bool;

    /// Helper method to mark a spawned batch as reversibly spawned. Useful when the actual spawn is
    /// hidden and cannot be done with [`World::rev_spawn_batch`].
    ///
    /// When possible, use `World::rev_spawn_batch` instead.
    ///
    /// See the [`RevDespawned`](super::RevDespawned) documentation to understand the mechanics of
    /// reversible spawn/despawn.
    fn rev_mark_spawned_batch(
        &mut self,
        meta_past_len: MetaPastLen,
        entities: &[Entity],
        include_unlinked_related: bool,
    );

    /// Reversible version of [`World::despawn`].
    ///
    /// See the [`RevDespawned`](super::RevDespawned) documentation to understand the mechanics of
    /// reversible spawn/despawn.
    fn rev_despawn(&mut self, meta_past_len: MetaPastLen, entity: Entity) -> bool;

    /// Reversibly despawn multiple entities.
    ///
    /// See the [`RevDespawned`](super::RevDespawned) documentation to understand the mechanics of
    /// reversible spawn/despawn.
    fn rev_despawn_batch(&mut self, meta_past_len: MetaPastLen, entities: &[Entity]);

    /// Reversible version of [`World::spawn`].
    ///
    /// See the [`RevDespawned`](super::RevDespawned) documentation to understand the mechanics of
    /// reversible spawn/despawn.
    fn rev_spawn(&mut self, meta_past_len: MetaPastLen, bundle: impl Bundle) -> EntityWorldMut<'_>;

    /// Reversible version of [`World::spawn_at`].
    ///
    /// See the [`RevDespawned`](super::RevDespawned) documentation to understand the mechanics of
    /// reversible spawn/despawn.
    fn rev_spawn_at(
        &mut self,
        meta_past_len: MetaPastLen,
        entity: Entity,
        bundle: impl Bundle,
    ) -> Result<EntityWorldMut<'_>, SpawnError>;

    /// Reversible version of [`World::spawn_empty`].
    ///
    /// See the [`RevDespawned`](super::RevDespawned) documentation to understand the mechanics of
    /// reversible spawn/despawn.
    fn rev_spawn_empty(&mut self, meta_past_len: MetaPastLen) -> EntityWorldMut<'_>;

    /// Reversible version of [`World::spawn_empty_at`].
    ///
    /// See the [`RevDespawned`](super::RevDespawned) documentation to understand the mechanics of
    /// reversible spawn/despawn.
    fn rev_spawn_empty_at(
        &mut self,
        meta_past_len: MetaPastLen,
        entity: Entity,
    ) -> Result<EntityWorldMut<'_>, SpawnError>;

    /// Reversible version of [`World::spawn_batch`].
    ///
    /// See the [`RevDespawned`](super::RevDespawned) documentation to understand the mechanics of
    /// reversible spawn/despawn.
    fn rev_spawn_batch<I>(&mut self, meta_past_len: MetaPastLen, iter: I) -> Vec<Entity>
    where
        I: IntoIterator<Item: Bundle<Effect: NoBundleEffect>>;

    /// Reversible version of [`World::insert_batch`].
    fn rev_insert_batch<I, B, Marker>(
        &mut self,
        meta_past_len: MetaPastLen,
        iter: I,
        caller: MaybeLocation,
    ) where
        I: IntoIterator<IntoIter: Iterator<Item = (Entity, B)>>,
        B: RevBundle<Marker>;

    /// Reversible version of [`World::insert_batch_if_new`].
    fn rev_insert_batch_if_new<I, B, Marker>(
        &mut self,
        meta_past_len: MetaPastLen,
        iter: I,
        caller: MaybeLocation,
    ) where
        I: IntoIterator<IntoIter: Iterator<Item = (Entity, B)>>,
        B: RevBundle<Marker>;

    /// Reversible version of [`World::try_insert_batch`].
    fn rev_try_insert_batch<I, B, Marker>(
        &mut self,
        meta_past_len: MetaPastLen,
        iter: I,
        caller: MaybeLocation,
    ) -> Result<(), TryRevInsertBatchError>
    where
        I: IntoIterator<IntoIter: Iterator<Item = (Entity, B)>>,
        B: RevBundle<Marker>;

    /// Reversible version of [`World::try_insert_batch_if_new`].
    fn rev_try_insert_batch_if_new<I, B, Marker>(
        &mut self,
        meta_past_len: MetaPastLen,
        iter: I,
        caller: MaybeLocation,
    ) -> Result<(), TryRevInsertBatchError>
    where
        I: IntoIterator<IntoIter: Iterator<Item = (Entity, B)>>,
        B: RevBundle<Marker>;

    /// Reversible version of [`World::get_resource_or_init`].
    fn rev_get_resource_or_init<R: Resource + FromWorld>(
        &mut self,
        meta_past_len: MetaPastLen,
    ) -> Mut<'_, R>;

    /// Reversible version of [`World::get_resource_or_insert_with`].
    fn rev_get_resource_or_insert_with<R: Resource>(
        &mut self,
        meta_past_len: MetaPastLen,
        func: impl FnOnce() -> R,
    ) -> Mut<'_, R>;

    /// Reversible version of [`World::init_resource`].
    fn rev_init_resource<R: Resource + FromWorld>(
        &mut self,
        meta_past_len: MetaPastLen,
    ) -> ComponentId;

    /// Reversible version of [`World::insert_resource`].
    fn rev_insert_resource<R: Resource>(&mut self, meta_past_len: MetaPastLen, resource: R);

    /// Reversible version of [`World::remove_resource`].
    fn rev_remove_resource<R: Resource, Out>(
        &mut self,
        meta_past_len: MetaPastLen,
        c: impl FnOnce(&R) -> Out,
    ) -> Option<Out>;
}

impl RevWorld for World {
    #[track_caller]
    fn redo_and_buffer(&mut self, meta_past_len: MetaPastLen, undo_redo: impl UndoRedo) {
        self.redo_and_buffer_with_caller(meta_past_len, undo_redo, MaybeLocation::caller());
    }

    fn get_running_direction(&self) -> Option<RevDirection> {
        self.get_resource::<RevMeta>()?.get_running_direction()
    }

    #[track_caller]
    fn rev_mark_spawned(
        &mut self,
        meta_past_len: MetaPastLen,
        entity: Entity,
        include_unlinked_related: bool,
    ) -> bool {
        self.rev_mark_spawned_with_caller(
            meta_past_len,
            entity,
            include_unlinked_related,
            MaybeLocation::caller(),
        )
    }

    #[track_caller]
    fn rev_mark_spawned_batch(
        &mut self,
        meta_past_len: MetaPastLen,
        entities: &[Entity],
        include_unlinked_related: bool,
    ) {
        self.rev_mark_spawned_batch_with_caller(
            meta_past_len,
            entities,
            include_unlinked_related,
            MaybeLocation::caller(),
        );
    }

    #[track_caller]
    fn rev_despawn(&mut self, meta_past_len: MetaPastLen, entity: Entity) -> bool {
        self.rev_despawn_with_caller(meta_past_len, entity, MaybeLocation::caller())
    }

    #[track_caller]
    fn rev_despawn_batch(&mut self, meta_past_len: MetaPastLen, entities: &[Entity]) {
        self.rev_despawn_batch_with_caller(meta_past_len, entities, MaybeLocation::caller())
    }

    #[track_caller]
    fn rev_spawn(&mut self, meta_past_len: MetaPastLen, bundle: impl Bundle) -> EntityWorldMut<'_> {
        self.rev_spawn_with_caller(meta_past_len, bundle, MaybeLocation::caller())
    }

    #[track_caller]
    fn rev_spawn_at(
        &mut self,
        meta_past_len: MetaPastLen,
        entity: Entity,
        bundle: impl Bundle,
    ) -> Result<EntityWorldMut<'_>, SpawnError> {
        self.rev_spawn_at_with_caller(meta_past_len, entity, bundle, MaybeLocation::caller())
    }

    #[track_caller]
    fn rev_spawn_empty(&mut self, meta_past_len: MetaPastLen) -> EntityWorldMut<'_> {
        self.rev_spawn_empty_with_caller(meta_past_len, MaybeLocation::caller())
    }

    #[track_caller]
    fn rev_spawn_empty_at(
        &mut self,
        meta_past_len: MetaPastLen,
        entity: Entity,
    ) -> Result<EntityWorldMut<'_>, SpawnError> {
        self.rev_spawn_empty_at_with_caller(meta_past_len, entity, MaybeLocation::caller())
    }

    #[track_caller]
    fn rev_spawn_batch<I>(&mut self, meta_past_len: MetaPastLen, iter: I) -> Vec<Entity>
    where
        I: IntoIterator<Item: Bundle<Effect: NoBundleEffect>>,
    {
        self.rev_spawn_batch_with_caller(meta_past_len, iter, MaybeLocation::caller())
    }

    #[track_caller]
    fn rev_get_resource_or_init<R: Resource + FromWorld>(
        &mut self,
        meta_past_len: MetaPastLen,
    ) -> Mut<'_, R> {
        self.rev_get_resource_or_init_with_caller(meta_past_len, MaybeLocation::caller())
    }

    #[track_caller]
    fn rev_get_resource_or_insert_with<R: Resource>(
        &mut self,
        meta_past_len: MetaPastLen,
        func: impl FnOnce() -> R,
    ) -> Mut<'_, R> {
        self.rev_get_resource_or_insert_with_with_caller(
            meta_past_len,
            func,
            MaybeLocation::caller(),
        )
    }

    #[track_caller]
    fn rev_init_resource<R: Resource + FromWorld>(
        &mut self,
        meta_past_len: MetaPastLen,
    ) -> ComponentId {
        self.rev_init_resource_with_caller::<R>(meta_past_len, MaybeLocation::caller())
    }

    #[track_caller]
    fn rev_insert_resource<R: Resource>(&mut self, meta_past_len: MetaPastLen, resource: R) {
        self.rev_insert_resource_with_caller(meta_past_len, resource, MaybeLocation::caller());
    }

    #[track_caller]
    fn rev_remove_resource<R: Resource, Out>(
        &mut self,
        meta_past_len: MetaPastLen,
        c: impl FnOnce(&R) -> Out,
    ) -> Option<Out> {
        self.rev_remove_resource_with_caller(meta_past_len, c, MaybeLocation::caller())
    }

    #[track_caller]
    fn rev_insert_batch<I, B, Marker>(
        &mut self,
        meta_past_len: MetaPastLen,
        iter: I,
        caller: MaybeLocation,
    ) where
        I: IntoIterator<IntoIter: Iterator<Item = (Entity, B)>>,
        B: RevBundle<Marker>,
    {
        self.rev_try_insert_batch(meta_past_len, iter, caller)
            .unwrap()
    }

    #[track_caller]
    fn rev_insert_batch_if_new<I, B, Marker>(
        &mut self,
        meta_past_len: MetaPastLen,
        iter: I,
        caller: MaybeLocation,
    ) where
        I: IntoIterator<IntoIter: Iterator<Item = (Entity, B)>>,
        B: RevBundle<Marker>,
    {
        self.rev_try_insert_batch_if_new(meta_past_len, iter, caller)
            .unwrap()
    }

    #[track_caller]
    fn rev_try_insert_batch<I, B, Marker>(
        &mut self,
        meta_past_len: MetaPastLen,
        iter: I,
        caller: MaybeLocation,
    ) -> Result<(), TryRevInsertBatchError>
    where
        I: IntoIterator<IntoIter: Iterator<Item = (Entity, B)>>,
        B: RevBundle<Marker>,
    {
        self.rev_try_insert_batch_inner(iter, |mut entity_mut, bundle| {
            entity_mut
                .rev_insert_with_caller(meta_past_len, bundle, caller)
                .map(|_| ())
        })
    }

    #[track_caller]
    fn rev_try_insert_batch_if_new<I, B, Marker>(
        &mut self,
        meta_past_len: MetaPastLen,
        iter: I,
        caller: MaybeLocation,
    ) -> Result<(), TryRevInsertBatchError>
    where
        I: IntoIterator<IntoIter: Iterator<Item = (Entity, B)>>,
        B: RevBundle<Marker>,
    {
        self.rev_try_insert_batch_inner(iter, |mut entity_mut, bundle| {
            entity_mut
                .rev_insert_if_new_with_caller(meta_past_len, bundle, caller)
                .map(|_| ())
        })
    }
}

pub(super) trait RevWorldInternal {
    fn redo_and_buffer_with_caller(
        &mut self,
        meta_past_len: MetaPastLen,
        undo_redo: impl UndoRedo,
        caller: MaybeLocation,
    );

    fn rev_mark_spawned_with_caller(
        &mut self,
        meta_past_len: MetaPastLen,
        entity: Entity,
        include_unlinked_related: bool,
        caller: MaybeLocation,
    ) -> bool;

    fn rev_mark_spawned_batch_with_caller(
        &mut self,
        meta_past_len: MetaPastLen,
        entities: &[Entity],
        include_unlinked_related: bool,
        caller: MaybeLocation,
    );

    fn rev_despawn_with_caller(
        &mut self,
        meta_past_len: MetaPastLen,
        entity: Entity,
        caller: MaybeLocation,
    ) -> bool;

    fn rev_despawn_batch_with_caller(
        &mut self,
        meta_past_len: MetaPastLen,
        entities: &[Entity],
        caller: MaybeLocation,
    );

    fn rev_spawn_with_caller(
        &mut self,
        meta_past_len: MetaPastLen,
        bundle: impl Bundle,
        caller: MaybeLocation,
    ) -> EntityWorldMut<'_>;

    fn rev_spawn_at_with_caller(
        &mut self,
        meta_past_len: MetaPastLen,
        entity: Entity,
        bundle: impl Bundle,
        caller: MaybeLocation,
    ) -> Result<EntityWorldMut<'_>, SpawnError>;

    fn rev_spawn_empty_with_caller(
        &mut self,
        meta_past_len: MetaPastLen,
        caller: MaybeLocation,
    ) -> EntityWorldMut<'_>;

    fn rev_spawn_empty_at_with_caller(
        &mut self,
        meta_past_len: MetaPastLen,
        entity: Entity,
        caller: MaybeLocation,
    ) -> Result<EntityWorldMut<'_>, SpawnError>;

    fn rev_spawn_batch_with_caller<I>(
        &mut self,
        meta_past_len: MetaPastLen,
        iter: I,
        caller: MaybeLocation,
    ) -> Vec<Entity>
    where
        I: IntoIterator<Item: Bundle<Effect: NoBundleEffect>>;

    fn rev_try_insert_batch_inner<I, B, Marker>(
        &mut self,
        iter: I,
        op: impl FnMut(EntityWorldMut, B) -> Result<(), EntityRevDespawnedError>,
    ) -> Result<(), TryRevInsertBatchError>
    where
        I: IntoIterator<IntoIter: Iterator<Item = (Entity, B)>>,
        B: RevBundle<Marker>;

    fn rev_get_resource_or_init_with_caller<R: Resource + FromWorld>(
        &mut self,
        meta_past_len: MetaPastLen,
        caller: MaybeLocation,
    ) -> Mut<'_, R>;

    fn rev_get_resource_or_insert_with_with_caller<R: Resource>(
        &mut self,
        meta_past_len: MetaPastLen,
        func: impl FnOnce() -> R,
        caller: MaybeLocation,
    ) -> Mut<'_, R>;

    fn rev_init_resource_with_caller<R: Resource + FromWorld>(
        &mut self,
        meta_past_len: MetaPastLen,
        caller: MaybeLocation,
    ) -> ComponentId;

    fn rev_insert_resource_with_caller<R: Resource>(
        &mut self,
        meta_past_len: MetaPastLen,
        resource: R,
        caller: MaybeLocation,
    );

    fn rev_remove_resource_with_caller<R: Resource, Out>(
        &mut self,
        meta_past_len: MetaPastLen,
        c: impl FnOnce(&R) -> Out,
        caller: MaybeLocation,
    ) -> Option<Out>;
}

impl RevWorldInternal for World {
    fn redo_and_buffer_with_caller(
        &mut self,
        meta_past_len: MetaPastLen,
        mut undo_redo: impl UndoRedo,
        caller: MaybeLocation,
    ) {
        undo_redo.redo(self);
        self.buffer_undo_redo_with_caller(meta_past_len, undo_redo, caller)
    }

    fn rev_mark_spawned_with_caller(
        &mut self,
        meta_past_len: MetaPastLen,
        entity: Entity,
        include_unlinked_related: bool,
        caller: MaybeLocation,
    ) -> bool {
        let Ok(mut entity) = self.get_entity_mut(entity) else {
            return false;
        };
        mark_entity::<true>(meta_past_len, &mut entity, include_unlinked_related, caller)
    }

    fn rev_mark_spawned_batch_with_caller(
        &mut self,
        meta_past_len: MetaPastLen,
        entities: &[Entity],
        include_unlinked_related: bool,
        caller: MaybeLocation,
    ) {
        mark_entities::<true>(
            meta_past_len,
            self,
            entities,
            include_unlinked_related,
            caller,
        );
    }

    fn rev_despawn_with_caller(
        &mut self,
        meta_past_len: MetaPastLen,
        entity: Entity,
        caller: MaybeLocation,
    ) -> bool {
        let Ok(mut entity) = self.get_entity_mut(entity) else {
            return false;
        };
        mark_entity::<false>(meta_past_len, &mut entity, false, caller)
    }

    fn rev_despawn_batch_with_caller(
        &mut self,
        meta_past_len: MetaPastLen,
        entities: &[Entity],
        caller: MaybeLocation,
    ) {
        mark_entities::<false>(meta_past_len, self, entities, false, caller);
    }

    fn rev_get_resource_or_init_with_caller<R: Resource + FromWorld>(
        &mut self,
        meta_past_len: MetaPastLen,
        caller: MaybeLocation,
    ) -> Mut<'_, R> {
        self.rev_init_resource_with_caller::<R>(meta_past_len, caller);
        self.resource_mut::<R>()
    }

    fn rev_get_resource_or_insert_with_with_caller<R: Resource>(
        &mut self,
        meta_past_len: MetaPastLen,
        func: impl FnOnce() -> R,
        caller: MaybeLocation,
    ) -> Mut<'_, R> {
        if !self.contains_resource::<R>() {
            self.buffer_undo_redo_with_caller(
                meta_past_len,
                RevInsertResourceNew::<R>::new(caller),
                caller,
            );
        }
        self.get_resource_or_insert_with(func)
    }

    fn rev_init_resource_with_caller<R: Resource + FromWorld>(
        &mut self,
        meta_past_len: MetaPastLen,
        caller: MaybeLocation,
    ) -> ComponentId {
        if !self.contains_resource::<R>() {
            self.buffer_undo_redo_with_caller(
                meta_past_len,
                RevInsertResourceNew::<R>::new(caller),
                caller,
            );
        }
        self.init_resource::<R>()
    }

    fn rev_insert_resource_with_caller<R: Resource>(
        &mut self,
        meta_past_len: MetaPastLen,
        resource: R,
        caller: MaybeLocation,
    ) {
        match self.remove_resource::<R>() {
            Some(resource) => self.buffer_undo_redo_with_caller(
                meta_past_len,
                RevInsertResourceOverwrite::new(resource, caller),
                caller,
            ),
            None => self.buffer_undo_redo_with_caller(
                meta_past_len,
                RevInsertResourceNew::<R>::new(caller),
                caller,
            ),
        }
        self.insert_resource(resource);
    }

    fn rev_remove_resource_with_caller<R: Resource, Out>(
        &mut self,
        meta_past_len: MetaPastLen,
        c: impl FnOnce(&R) -> Out,
        caller: MaybeLocation,
    ) -> Option<Out> {
        self.remove_resource::<R>().map(|resource| {
            let out = c(&resource);
            self.buffer_undo_redo_with_caller(
                meta_past_len,
                RevRemoveResource::new(resource, caller),
                caller,
            );
            out
        })
    }

    fn rev_spawn_with_caller(
        &mut self,
        meta_past_len: MetaPastLen,
        bundle: impl Bundle,
        caller: MaybeLocation,
    ) -> EntityWorldMut<'_> {
        let mut entity = self.spawn(bundle);
        entity
            .rev_mark_spawned_with_caller(meta_past_len, true, caller)
            .unwrap();
        entity
    }

    fn rev_spawn_at_with_caller(
        &mut self,
        meta_past_len: MetaPastLen,
        entity: Entity,
        bundle: impl Bundle,
        caller: MaybeLocation,
    ) -> Result<EntityWorldMut<'_>, SpawnError> {
        let mut entity = self.spawn_at(entity, bundle)?;
        entity
            .rev_mark_spawned_with_caller(meta_past_len, true, caller)
            .unwrap();
        Ok(entity)
    }

    fn rev_spawn_empty_with_caller(
        &mut self,
        meta_past_len: MetaPastLen,
        caller: MaybeLocation,
    ) -> EntityWorldMut<'_> {
        let mut entity = self.spawn_empty();
        mark_spawn_empty(meta_past_len, &mut entity, caller);
        entity
    }

    fn rev_spawn_empty_at_with_caller(
        &mut self,
        meta_past_len: MetaPastLen,
        entity: Entity,
        caller: MaybeLocation,
    ) -> Result<EntityWorldMut<'_>, SpawnError> {
        let mut entity = self.spawn_empty_at(entity)?;
        entity
            .rev_mark_spawned_with_caller(meta_past_len, true, caller)
            .unwrap();
        Ok(entity)
    }

    fn rev_spawn_batch_with_caller<I>(
        &mut self,
        meta_past_len: MetaPastLen,
        iter: I,
        caller: MaybeLocation,
    ) -> Vec<Entity>
    where
        I: IntoIterator<Item: Bundle<Effect: NoBundleEffect>>,
    {
        let entities = self.spawn_batch(iter).collect::<Vec<_>>();
        mark_entities::<true>(meta_past_len, self, &*entities, true, caller);
        entities
    }

    fn rev_try_insert_batch_inner<I, B, Marker>(
        &mut self,
        iter: I,
        mut op: impl FnMut(EntityWorldMut, B) -> Result<(), EntityRevDespawnedError>,
    ) -> Result<(), TryRevInsertBatchError>
    where
        I: IntoIterator<IntoIter: Iterator<Item = (Entity, B)>>,
        B: RevBundle<Marker>,
    {
        let mut not_existing_entities = Vec::new();
        let mut rev_despawned_entities = Vec::new();
        for (entity, bundle) in iter.into_iter() {
            match self.get_entity_mut(entity) {
                Ok(entity_mut) => {
                    if let Err(err) = op(entity_mut, bundle) {
                        rev_despawned_entities.push(err);
                    }
                }
                Err(EntityMutableFetchError::NotSpawned(err)) => {
                    not_existing_entities.push(err);
                }
                Err(EntityMutableFetchError::AliasedMutability(_)) => unreachable!(),
            }
        }

        if not_existing_entities.is_empty() && rev_despawned_entities.is_empty() {
            Ok(())
        } else {
            Err(TryRevInsertBatchError {
                bundle_type: DebugName::type_name::<B>(),
                not_existing_entities,
                rev_despawned_entities,
            })
        }
    }
}

/// The error type returned by [`World::rev_try_insert_batch`] and
/// [`World::rev_try_insert_batch_if_new`] if any of the provided entities do not exist or were
/// reversibly despawned.
///
/// See the [`RevDespawned`](super::RevDespawned) documentation to understand the mechanics of
/// reversible spawn/despawn.
#[derive(Debug, Clone)]
pub struct TryRevInsertBatchError {
    /// The bundles' type name.
    pub bundle_type: DebugName,
    /// The IDs of the provided entities that do not exist.
    pub not_existing_entities: Vec<EntityNotSpawnedError>,
    /// The IDs of the provided entities that are reversibly despawned.
    pub rev_despawned_entities: Vec<EntityRevDespawnedError>,
}

impl Display for TryRevInsertBatchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.rev_despawned_entities.is_empty() {
            write!(
                f,
                "Could not insert bundles of type {} into the entities with the following IDs because they do not exist: {:?}",
                self.bundle_type, self.not_existing_entities
            )
        } else if self.not_existing_entities.is_empty() {
            write!(
                f,
                "Could not insert bundles of type {} into the entities with the following IDs because they were reversibly despawned: {:?}",
                self.bundle_type, self.rev_despawned_entities
            )
        } else {
            write!(
                f,
                "Could not insert bundles of type {} into the entities with the following IDs because they do not exist: {:?} or were reversibly despawned: {:?}",
                self.bundle_type, self.not_existing_entities, self.rev_despawned_entities
            )
        }
    }
}

impl Error for TryRevInsertBatchError {}

use core::{error::Error, fmt::Display};

use bevy_ecs::{
    bundle::{Bundle, NoBundleEffect},
    change_detection::MaybeLocation,
    component::ComponentId,
    entity::{Entity, EntityNotSpawnedError},
    resource::Resource,
    schedule::{InternedScheduleLabel, ScheduleLabel},
    world::{
        EntityWorldMut, FromWorld, World,
        error::{EntityMutableFetchError, TryRunScheduleError},
    },
};
use bevy_utils::prelude::DebugName;

use crate::{
    meta::MetaPastLen,
    undo_redo::{
        BuffersUndoRedo, EntityRevDespawnedError, RevBundle, RevInsertResourceNew,
        RevInsertResourceOverwrite, RevRemoveResource, UndoRedo, mark_entities, mark_entity,
    },
};

pub(super) trait RevWorld {
    fn rev_try_run_schedule_with_caller(
        &mut self,
        meta_past_len: MetaPastLen,
        label: impl ScheduleLabel,
        caller: MaybeLocation,
    ) -> Result<(), TryRunScheduleError>;

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

impl RevWorld for World {
    fn rev_try_run_schedule_with_caller(
        &mut self,
        meta_past_len: MetaPastLen,
        label: impl ScheduleLabel,
        caller: MaybeLocation,
    ) -> Result<(), TryRunScheduleError> {
        let label = label.intern();
        self.try_run_schedule(label).inspect(move |()| {
            self.buffer_undo_redo_with_caller(meta_past_len, RevRunSchedule(label), caller);
        })
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

struct RevRunSchedule(InternedScheduleLabel);

impl UndoRedo for RevRunSchedule {
    fn undo(&mut self, world: &mut World) {
        world.run_schedule(self.0);
    }
    fn redo(&mut self, world: &mut World) {
        world.run_schedule(self.0);
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
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
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

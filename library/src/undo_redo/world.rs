use alloc::vec::Vec;
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
use core::{error::Error, fmt::Display};

use crate::undo_redo::{
    EntityRevDespawnedError, RevBundle, RevInsertResourceNew, RevInsertResourceOverwrite,
    RevRemoveResource, UndoRedo, UndoRedoQueue, mark_entities, mark_entity,
};

pub(super) trait RevWorld {
    fn queue_undo_redo(&mut self, undo_redo: impl UndoRedo, caller: MaybeLocation);

    fn redo_and_queue(&mut self, undo_redo: impl UndoRedo, caller: MaybeLocation);

    fn rev_try_run_schedule(
        &mut self,
        label: impl ScheduleLabel,
        caller: MaybeLocation,
    ) -> Result<(), TryRunScheduleError>;

    fn rev_mark_spawned(
        &mut self,
        entity: Entity,
        include_unlinked_related: bool,
        caller: MaybeLocation,
    ) -> bool;

    fn rev_mark_spawned_batch(
        &mut self,
        entities: &[Entity],
        include_unlinked_related: bool,
        caller: MaybeLocation,
    );

    fn rev_despawn(&mut self, entity: Entity, caller: MaybeLocation) -> bool;

    fn rev_despawn_batch(&mut self, entities: &[Entity], caller: MaybeLocation);

    fn rev_spawn_batch<I>(&mut self, iter: I, caller: MaybeLocation) -> Vec<Entity>
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

    fn rev_init_resource<R: Resource + FromWorld>(&mut self, caller: MaybeLocation) -> ComponentId;

    fn rev_insert_resource<R: Resource>(&mut self, resource: R, caller: MaybeLocation);

    fn rev_remove_resource<R: Resource, Out>(
        &mut self,
        c: impl FnOnce(&R) -> Out,
        caller: MaybeLocation,
    ) -> Option<Out>;
}

impl RevWorld for World {
    fn queue_undo_redo(&mut self, undo_redo: impl UndoRedo, caller: MaybeLocation) {
        self.get_resource_or_init::<UndoRedoQueue>()
            .queue_undo_redo(caller, undo_redo);
    }

    fn redo_and_queue(&mut self, mut undo_redo: impl UndoRedo, caller: MaybeLocation) {
        undo_redo.redo(self);
        self.queue_undo_redo(undo_redo, caller);
    }

    fn rev_try_run_schedule(
        &mut self,
        label: impl ScheduleLabel,
        caller: MaybeLocation,
    ) -> Result<(), TryRunScheduleError> {
        let label = label.intern();
        self.try_run_schedule(label).inspect(move |()| {
            self.queue_undo_redo(RevRunSchedule(label), caller);
        })
    }

    fn rev_mark_spawned(
        &mut self,
        entity: Entity,
        include_unlinked_related: bool,
        caller: MaybeLocation,
    ) -> bool {
        let Ok(mut entity) = self.get_entity_mut(entity) else {
            return false;
        };
        mark_entity::<true>(&mut entity, include_unlinked_related, caller)
    }

    fn rev_mark_spawned_batch(
        &mut self,
        entities: &[Entity],
        include_unlinked_related: bool,
        caller: MaybeLocation,
    ) {
        mark_entities::<true>(self, entities, include_unlinked_related, caller);
    }

    fn rev_despawn(&mut self, entity: Entity, caller: MaybeLocation) -> bool {
        let Ok(mut entity) = self.get_entity_mut(entity) else {
            return false;
        };
        mark_entity::<false>(&mut entity, false, caller)
    }

    fn rev_despawn_batch(&mut self, entities: &[Entity], caller: MaybeLocation) {
        mark_entities::<false>(self, entities, false, caller);
    }

    fn rev_init_resource<R: Resource + FromWorld>(&mut self, caller: MaybeLocation) -> ComponentId {
        if !self.contains_resource::<R>() {
            self.queue_undo_redo(RevInsertResourceNew::<R>::new(caller), caller);
        }
        self.init_resource::<R>()
    }

    fn rev_insert_resource<R: Resource>(&mut self, resource: R, caller: MaybeLocation) {
        match self.remove_resource::<R>() {
            Some(resource) => {
                self.queue_undo_redo(RevInsertResourceOverwrite::new(resource, caller), caller)
            }
            None => self.queue_undo_redo(RevInsertResourceNew::<R>::new(caller), caller),
        }
        self.insert_resource(resource);
    }

    fn rev_remove_resource<R: Resource, Out>(
        &mut self,
        c: impl FnOnce(&R) -> Out,
        caller: MaybeLocation,
    ) -> Option<Out> {
        self.remove_resource::<R>().map(|resource| {
            let out = c(&resource);
            self.queue_undo_redo(RevRemoveResource::new(resource, caller), caller);
            out
        })
    }

    fn rev_spawn_batch<I>(&mut self, iter: I, caller: MaybeLocation) -> Vec<Entity>
    where
        I: IntoIterator<Item: Bundle<Effect: NoBundleEffect>>,
    {
        let entities = self.spawn_batch(iter).collect::<Vec<_>>();
        mark_entities::<true>(self, &entities, true, caller);
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
                // only one entity is fetched
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

#[derive(Debug, Clone)]
pub(super) struct TryRevInsertBatchError {
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

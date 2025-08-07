use std::sync::Arc;

use bevy::{
    ecs::{
        bundle::{Bundle, NoBundleEffect},
        change_detection::MaybeLocation,
        component::ComponentId,
        entity::Entity,
        resource::Resource,
        world::{EntityWorldMut, FromWorld, Mut, World},
    },
    log::warn,
};

use crate::{meta::NonLogNow, undo_redo::RevDespawned};

use super::{
    BuffersUndoRedo, EntityRevDespawnedError, ResourceSwap, RevDespawnCleaner, RevDespawnedBy,
    RevEntityError, Spawn, UndoRedo, UndoRedoSwap, rev_spawn_finish,
};

pub trait RevWorld {
    fn redo_and_buffer(&mut self, now: NonLogNow, undo_redo: impl UndoRedo);

    // the methods here are purposely sorted alphabetically to make it easily comparable to bevy's docs
    // unmentioned methods are either
    // a) unrelated to reversible structural changes OR
    // b) deprecated in bevy OR
    // c) missed by accident!

    /// Reversible version of [`World::despawn`].
    fn rev_despawn(&mut self, now: NonLogNow, entity: Entity) -> bool;

    /// Reversible version of [`World::get_resource_or_init`].
    fn rev_get_resource_or_init<R: Resource + FromWorld>(&mut self, now: NonLogNow) -> Mut<'_, R>;

    /// Reversible version of [`World::get_resource_or_insert_with`].
    fn rev_get_resource_or_insert_with<R: Resource>(
        &mut self,
        now: NonLogNow,
        func: impl FnOnce() -> R,
    ) -> Mut<'_, R>;

    // rev_init_non_send_resource
    // out of scope due Send bound on UndoRedo

    /// Reversible version of [`World::init_resource`].
    fn rev_init_resource<R: Resource + FromWorld>(&mut self, now: NonLogNow) -> ComponentId;

    // rev_insert_batch
    // out of scope

    // rev_insert_batch_if_new
    // out of scope

    // rev_insert_non_send_by_id
    // out of scope due Send bound on UndoRedo

    // rev_insert_non_send_resource
    // out of scope due Send bound on UndoRedo

    /// Reversible version of [`World::insert_resource`].
    fn rev_insert_resource<R: Resource>(&mut self, now: NonLogNow, resource: R);

    // rev_insert_resource_by_id
    // blocked on https://github.com/bevyengine/bevy/pull/17485

    // rev_remove_non_send_by_id
    // out of scope due Send bound on UndoRedo

    /// Reversible version of [`World::remove_resource`].
    fn rev_remove_resource<R: Resource, Out>(
        &mut self,
        now: NonLogNow,
        c: impl FnOnce(&R) -> Out,
    ) -> Option<Out>;

    /// rev_remove_resource_by_id
    // blocked on https://github.com/bevyengine/bevy/pull/17485

    /// Reversible version of [`World::spawn`].
    fn rev_spawn<T: Bundle>(&mut self, now: NonLogNow, bundle: T) -> EntityWorldMut;

    /// Reversible version of [`World::spawn_batch`].
    fn rev_spawn_batch<I>(&mut self, now: NonLogNow, iter: I) -> Arc<[Entity]>
    where
        I: IntoIterator,
        I::Item: Bundle<Effect: NoBundleEffect>;

    /// Reversible version of [`World::spawn_empty`].
    fn rev_spawn_empty(&mut self, now: NonLogNow) -> EntityWorldMut;

    /// Reversible version of [`World::try_despawn`].
    fn rev_try_despawn(&mut self, now: NonLogNow, entity: Entity) -> Result<(), RevEntityError>;

    // rev_try_insert_batch
    // out of scope

    // rev_try_insert_batch_if_new
    // out of scope
}

impl RevWorld for World {
    fn redo_and_buffer(&mut self, now: NonLogNow, mut undo_redo: impl UndoRedo) {
        undo_redo.redo(self);
        self.buffer_undo_redo(now, undo_redo)
    }

    #[track_caller]
    fn rev_despawn(&mut self, now: NonLogNow, entity: Entity) -> bool {
        self.rev_try_despawn(now, entity)
            .inspect_err(|err| warn!("entity {entity} could not be reversibly despawned: {err}"))
            .is_ok()
    }

    fn rev_get_resource_or_init<R: Resource + FromWorld>(&mut self, now: NonLogNow) -> Mut<'_, R> {
        self.rev_init_resource::<R>(now);
        self.resource_mut::<R>()
    }

    fn rev_get_resource_or_insert_with<R: Resource>(
        &mut self,
        now: NonLogNow,
        func: impl FnOnce() -> R,
    ) -> Mut<'_, R> {
        if !self.contains_resource::<R>() {
            self.buffer_undo_redo(now, ResourceSwap::<R>(None));
        }
        self.get_resource_or_insert_with(func)
    }

    fn rev_init_resource<R: Resource + FromWorld>(&mut self, now: NonLogNow) -> ComponentId {
        if !self.contains_resource::<R>() {
            self.buffer_undo_redo(now, ResourceSwap::<R>(None));
        }
        self.init_resource::<R>()
    }

    fn rev_insert_resource<R: Resource>(&mut self, now: NonLogNow, resource: R) {
        let swap = ResourceSwap(self.remove_resource::<R>());
        self.insert_resource(resource);
        self.buffer_undo_redo(now, swap);
    }

    fn rev_remove_resource<R: Resource, Out>(
        &mut self,
        now: NonLogNow,
        c: impl FnOnce(&R) -> Out,
    ) -> Option<Out> {
        self.remove_resource::<R>().map(|resource| {
            let out = c(&resource);
            self.buffer_undo_redo(now, ResourceSwap(Some(resource)));
            out
        })
    }

    #[track_caller]
    fn rev_spawn<T: Bundle>(&mut self, now: NonLogNow, bundle: T) -> EntityWorldMut {
        let mut entity_world_mut = self.spawn(bundle);
        let entity = entity_world_mut.id();
        rev_spawn_finish(&mut entity_world_mut, now, entity, MaybeLocation::caller());
        entity_world_mut
    }

    #[track_caller]
    fn rev_spawn_batch<I>(&mut self, now: NonLogNow, iter: I) -> Arc<[Entity]>
    where
        I: IntoIterator,
        I::Item: Bundle<Effect: NoBundleEffect>,
    {
        struct SpawnBatch {
            entities: Arc<[Entity]>,
            location: MaybeLocation,
        }

        impl UndoRedo for SpawnBatch {
            fn undo(&mut self, world: &mut World) {
                world.insert_batch(
                    self.entities
                        .iter()
                        .map(|entity| (*entity, RevDespawned(self.location)))
                        .rev(),
                );
            }
            fn redo(&mut self, world: &mut World) {
                let id = world.register_component::<RevDespawned>();
                for entity in self.entities.iter() {
                    world.entity_mut(*entity).remove_by_id(id);
                }
            }
        }

        let entities: Arc<[Entity]> = self.spawn_batch(iter).collect();
        let location = MaybeLocation::caller();

        self.buffer_undo_redo(
            now,
            SpawnBatch {
                entities: entities.clone(),
                location,
            },
        );
        self.resource_mut::<RevDespawnCleaner>()
            .log_spawn_batch(&entities, location, now);

        entities
    }

    #[track_caller]
    fn rev_spawn_empty(&mut self, now: NonLogNow) -> EntityWorldMut {
        let mut entity_world_mut = self.spawn_empty();
        let entity = entity_world_mut.id();
        rev_spawn_finish(&mut entity_world_mut, now, entity, MaybeLocation::caller());
        entity_world_mut
    }

    #[track_caller]
    fn rev_try_despawn(&mut self, now: NonLogNow, entity: Entity) -> Result<(), RevEntityError> {
        if let Some(location) = self.get_entity(entity)?.get_rev_despawned_by() {
            return Err(RevEntityError::EntityRevDespawnedError(
                EntityRevDespawnedError { entity, location },
            ));
        }

        let location = MaybeLocation::caller();

        self.redo_and_buffer(now, UndoRedoSwap(Spawn { entity, location }));
        self.resource_mut::<RevDespawnCleaner>()
            .log_despawn(entity, location, now);

        Ok(())
    }
}

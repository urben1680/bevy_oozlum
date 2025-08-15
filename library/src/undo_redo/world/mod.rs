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

use crate::{
    meta::NonLogNow,
    undo_redo::{RevDespawned, RevIsDespawned},
};

use super::{
    BuffersUndoRedo, EntityRevDespawnedError, ResourceSwap, RevDespawnCleaner, RevEntityError,
    Spawn, UndoRedo, UndoRedoSwap, rev_spawn_finish,
};

#[cfg(test)]
mod test;

pub trait RevWorld {
    fn redo_and_buffer(&mut self, now: NonLogNow, undo_redo: impl UndoRedo);

    // the methods here are purposely sorted alphabetically to make it easily comparable to bevy's docs
    // unmentioned methods are either
    // a) unrelated to reversible structural changes OR
    // b) deprecated in bevy OR
    // c) missed by accident!

    /// Reversible version of [`World::despawn`].
    fn rev_despawn_single(&mut self, now: NonLogNow, entity: Entity) -> bool;

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
    // no efficient algorithm found yet

    // rev_insert_batch_if_new
    // no efficient algorithm found yet

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
    fn rev_spawn<T: Bundle<Effect: NoBundleEffect>>(
        &mut self,
        now: NonLogNow,
        bundle: T,
    ) -> EntityWorldMut;

    /// Reversible version of [`World::spawn_batch`].
    fn rev_spawn_batch<I>(&mut self, now: NonLogNow, iter: I) -> Arc<[Entity]>
    where
        I: IntoIterator,
        I::Item: Bundle<Effect: NoBundleEffect>;

    /// Reversible version of [`World::spawn_empty`].
    fn rev_spawn_empty(&mut self, now: NonLogNow) -> EntityWorldMut;

    /// Reversible version of [`World::try_despawn`].
    fn rev_try_despawn_single(&mut self, now: NonLogNow, entity: Entity) -> Result<(), RevEntityError>;

    // rev_try_insert_batch
    // no efficient algorithm found yet

    // rev_try_insert_batch_if_new
    // no efficient algorithm found yet
}

impl RevWorld for World {
    fn redo_and_buffer(&mut self, now: NonLogNow, mut undo_redo: impl UndoRedo) {
        undo_redo.redo(self);
        self.buffer_undo_redo(now, undo_redo)
    }

    #[track_caller]
    fn rev_despawn_single(&mut self, now: NonLogNow, entity: Entity) -> bool {
        self.rev_try_despawn_single(now, entity)
            .inspect_err(|err| warn!("entity {entity} could not be reversibly despawned: {err}"))
            .is_ok()
    }

    #[track_caller]
    fn rev_get_resource_or_init<R: Resource + FromWorld>(&mut self, now: NonLogNow) -> Mut<'_, R> {
        self.rev_init_resource::<R>(now);
        self.resource_mut::<R>()
    }

    #[track_caller]
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

    #[track_caller]
    fn rev_init_resource<R: Resource + FromWorld>(&mut self, now: NonLogNow) -> ComponentId {
        rev_init_resource_with_caller::<R>(self, now, MaybeLocation::caller())
    }

    #[track_caller]
    fn rev_insert_resource<R: Resource>(&mut self, now: NonLogNow, resource: R) {
        rev_insert_resource_with_caller(self, now, resource, MaybeLocation::caller())
    }

    #[track_caller]
    fn rev_remove_resource<R: Resource, Out>(
        &mut self,
        now: NonLogNow,
        c: impl FnOnce(&R) -> Out,
    ) -> Option<Out> {
        rev_remove_resource_with_caller(self, now, c, MaybeLocation::caller())
    }

    #[track_caller]
    fn rev_spawn<T: Bundle<Effect: NoBundleEffect>>(
        &mut self,
        now: NonLogNow,
        bundle: T,
    ) -> EntityWorldMut {
        rev_spawn_with_caller(self, now, bundle, MaybeLocation::caller())
    }

    #[track_caller]
    fn rev_spawn_batch<I>(&mut self, now: NonLogNow, iter: I) -> Arc<[Entity]>
    where
        I: IntoIterator,
        I::Item: Bundle<Effect: NoBundleEffect>,
    {
        rev_spawn_batch_with_caller(self, now, iter, MaybeLocation::caller())
    }

    #[track_caller]
    fn rev_spawn_empty(&mut self, now: NonLogNow) -> EntityWorldMut {
        rev_spawn_empty_with_caller(self, now, MaybeLocation::caller())
    }

    #[track_caller]
    fn rev_try_despawn_single(&mut self, now: NonLogNow, entity: Entity) -> Result<(), RevEntityError> {
        rev_try_despawn_single_with_caller(self, now, entity, MaybeLocation::caller())
    }
}

pub(crate) fn rev_init_resource_with_caller<R: Resource + FromWorld>(
    world: &mut World,
    now: NonLogNow,
    caller: MaybeLocation,
) -> ComponentId {
    if !world.contains_resource::<R>() {
        world.buffer_undo_redo(now, ResourceSwap::<R>(None));
    }
    world.init_resource::<R>()
}

pub(crate) fn rev_insert_resource_with_caller<R: Resource>(
    world: &mut World,
    now: NonLogNow,
    resource: R,
    caller: MaybeLocation,
) {
    let swap = ResourceSwap(world.remove_resource::<R>());
    world.insert_resource(resource);
    world.buffer_undo_redo(now, swap);
}

pub(crate) fn rev_remove_resource_with_caller<R: Resource, Out>(
    world: &mut World,
    now: NonLogNow,
    c: impl FnOnce(&R) -> Out,
    caller: MaybeLocation,
) -> Option<Out> {
    world.remove_resource::<R>().map(|resource| {
        let out = c(&resource);
        world.buffer_undo_redo(now, ResourceSwap(Some(resource)));
        out
    })
}

pub(crate) fn rev_spawn_with_caller<B: Bundle<Effect: NoBundleEffect>>(
    world: &mut World,
    now: NonLogNow,
    bundle: B,
    caller: MaybeLocation,
) -> EntityWorldMut {
    let mut entity_world_mut = world.spawn(bundle);
    let entity = entity_world_mut.id();
    rev_spawn_finish(&mut entity_world_mut, now, entity, MaybeLocation::caller());
    entity_world_mut
}

pub(crate) fn rev_spawn_batch_with_caller<I>(
    world: &mut World,
    now: NonLogNow,
    iter: I,
    caller: MaybeLocation,
) -> Arc<[Entity]>
where
    I: IntoIterator,
    I::Item: Bundle<Effect: NoBundleEffect>,
{
    struct SpawnBatch {
        entities: Arc<[Entity]>,
        spawn_location: MaybeLocation,
    }

    impl UndoRedo for SpawnBatch {
        fn undo(&mut self, world: &mut World) {
            world.insert_batch(
                self.entities
                    .iter()
                    .map(|entity| (*entity, RevDespawned))
                    // todo: set location of RevDespawned change meta https://github.com/bevyengine/bevy/issues/20494
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

    let entities: Arc<[Entity]> = world.spawn_batch(iter).collect();

    world.buffer_undo_redo(
        now,
        SpawnBatch {
            entities: entities.clone(),
            spawn_location: caller,
        },
    );
    world
        .resource_mut::<RevDespawnCleaner>()
        .log_spawn_batch(&entities, caller, now);

    entities
}

pub(crate) fn rev_spawn_empty_with_caller(
    world: &mut World,
    now: NonLogNow,
    caller: MaybeLocation,
) -> EntityWorldMut {
    let mut entity_world_mut = world.spawn_empty();
    let entity = entity_world_mut.id();
    rev_spawn_finish(&mut entity_world_mut, now, entity, caller);
    entity_world_mut
}

pub(crate) fn rev_try_despawn_single_with_caller(
    world: &mut World,
    now: NonLogNow,
    entity: Entity,
    caller: MaybeLocation,
) -> Result<(), RevEntityError> {
    if world.get_entity(entity)?.is_rev_despawned() {
        return Err(RevEntityError::EntityRevDespawnedError(
            EntityRevDespawnedError { entity },
        ));
    }

    world.redo_and_buffer(
        now,
        UndoRedoSwap(Spawn {
            entity,
            location: caller,
        }),
    );
    world
        .resource_mut::<RevDespawnCleaner>()
        .log_despawn(entity, caller, now);

    Ok(())
}

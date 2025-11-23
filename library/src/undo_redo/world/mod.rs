use super::{BuffersUndoRedo, ResourceSwap, RevDespawnCleaner, RevEntityError, UndoRedo};
use crate::{
    meta::{PastLen, RevDirection, RevMeta},
    undo_redo::{RevDespawned, rev_spawn_despawn_with_caller, rev_spawn_empty_inner},
};
use bevy_ecs::{
    bundle::{Bundle, NoBundleEffect},
    change_detection::MaybeLocation,
    component::ComponentId,
    entity::{Entity, EntityCloner},
    resource::Resource,
    world::{DeferredWorld, EntityWorldMut, FromWorld, Mut, World, error::EntityMutableFetchError},
};
use bevy_log::warn;
use std::sync::Arc;

#[cfg(test)]
mod test;

pub trait RevWorld {
    fn redo_and_buffer(&mut self, past_len: PastLen, undo_redo: impl UndoRedo);

    fn get_running_direction(&self) -> Option<RevDirection>;

    #[track_caller]
    fn rev_log_scope(&mut self, past_len: PastLen, entity: Entity);

    // the methods here are purposely sorted alphabetically to make it easily comparable to bevy's docs
    // unmentioned methods are either
    // a) unrelated to reversible structural changes OR
    // b) deprecated in bevy OR
    // c) missed by accident!

    /// Reversible version of [`World::despawn`].
    fn rev_despawn_single(&mut self, past_len: PastLen, entity: Entity) -> bool;

    /// Reversible version of [`World::get_resource_or_init`].
    fn rev_get_resource_or_init<R: Resource + FromWorld>(
        &mut self,
        past_len: PastLen,
    ) -> Mut<'_, R>;

    /// Reversible version of [`World::get_resource_or_insert_with`].
    fn rev_get_resource_or_insert_with<R: Resource>(
        &mut self,
        past_len: PastLen,
        func: impl FnOnce() -> R,
    ) -> Mut<'_, R>;

    // rev_init_non_send_resource
    // out of scope due Send bound on UndoRedo

    /// Reversible version of [`World::init_resource`].
    fn rev_init_resource<R: Resource + FromWorld>(&mut self, past_len: PastLen) -> ComponentId;

    // rev_insert_batch
    // no efficient algorithm found yet

    // rev_insert_batch_if_new
    // no efficient algorithm found yet

    // rev_insert_non_send_by_id
    // out of scope due Send bound on UndoRedo

    // rev_insert_non_send_resource
    // out of scope due Send bound on UndoRedo

    /// Reversible version of [`World::insert_resource`].
    fn rev_insert_resource<R: Resource>(&mut self, past_len: PastLen, resource: R);

    // rev_insert_resource_by_id
    // blocked on https://github.com/bevyengine/bevy/pull/17485

    // rev_remove_non_send_by_id
    // out of scope due Send bound on UndoRedo

    /// Reversible version of [`World::remove_resource`].
    fn rev_remove_resource<R: Resource, Out>(
        &mut self,
        past_len: PastLen,
        c: impl FnOnce(&R) -> Out,
    ) -> Option<Out>;

    /// rev_remove_resource_by_id
    // blocked on https://github.com/bevyengine/bevy/pull/17485

    /// Reversible version of [`World::spawn`].
    fn rev_spawn<T: Bundle<Effect: NoBundleEffect>>(
        &mut self,
        past_len: PastLen,
        bundle: T,
    ) -> EntityWorldMut;

    /// Reversible version of [`World::spawn_batch`].
    fn rev_spawn_batch<I>(&mut self, past_len: PastLen, iter: I) -> Arc<[Entity]>
    where
        I: IntoIterator,
        I::Item: Bundle<Effect: NoBundleEffect>;

    /// Reversible version of [`World::spawn_empty`].
    fn rev_spawn_empty(&mut self, past_len: PastLen) -> EntityWorldMut;

    /// Reversible version of [`World::try_despawn`].
    fn rev_try_despawn_single(
        &mut self,
        past_len: PastLen,
        entity: Entity,
    ) -> Result<(), RevEntityError>;

    // rev_try_insert_batch
    // no efficient algorithm found yet

    // rev_try_insert_batch_if_new
    // no efficient algorithm found yet
}

impl RevWorld for World {
    fn redo_and_buffer(&mut self, past_len: PastLen, mut undo_redo: impl UndoRedo) {
        // todo: pass location
        undo_redo.redo(self);
        self.buffer_undo_redo(past_len, undo_redo)
    }

    fn get_running_direction(&self) -> Option<RevDirection> {
        self.get_resource::<RevMeta>()?.get_running_direction()
    }

    #[track_caller]
    fn rev_log_scope(&mut self, past_len: PastLen, entity: Entity) {
        self.resource_mut::<RevDespawnCleaner>().log_spawn(
            entity,
            MaybeLocation::caller(),
            past_len,
        );
    }

    #[track_caller]
    fn rev_despawn_single(&mut self, past_len: PastLen, entity: Entity) -> bool {
        self.rev_try_despawn_single(past_len, entity)
            .inspect_err(|err| warn!("entity {entity} could not be reversibly despawned: {err}"))
            .is_ok()
    }

    #[track_caller]
    fn rev_get_resource_or_init<R: Resource + FromWorld>(
        &mut self,
        past_len: PastLen,
    ) -> Mut<'_, R> {
        self.rev_init_resource::<R>(past_len);
        self.resource_mut::<R>()
    }

    #[track_caller]
    fn rev_get_resource_or_insert_with<R: Resource>(
        &mut self,
        past_len: PastLen,
        func: impl FnOnce() -> R,
    ) -> Mut<'_, R> {
        if !self.contains_resource::<R>() {
            self.buffer_undo_redo(past_len, ResourceSwap::<R>(None));
        }
        self.get_resource_or_insert_with(func)
    }

    #[track_caller]
    fn rev_init_resource<R: Resource + FromWorld>(&mut self, past_len: PastLen) -> ComponentId {
        rev_init_resource_with_caller::<R>(self, past_len, MaybeLocation::caller())
    }

    #[track_caller]
    fn rev_insert_resource<R: Resource>(&mut self, past_len: PastLen, resource: R) {
        rev_insert_resource_with_caller(self, past_len, resource, MaybeLocation::caller())
    }

    #[track_caller]
    fn rev_remove_resource<R: Resource, Out>(
        &mut self,
        past_len: PastLen,
        c: impl FnOnce(&R) -> Out,
    ) -> Option<Out> {
        rev_remove_resource_with_caller(self, past_len, c, MaybeLocation::caller())
    }

    #[track_caller]
    fn rev_spawn<T: Bundle<Effect: NoBundleEffect>>(
        &mut self,
        past_len: PastLen,
        bundle: T,
    ) -> EntityWorldMut {
        rev_spawn_with_caller_world(self, past_len, bundle, MaybeLocation::caller())
    }

    #[track_caller]
    fn rev_spawn_batch<I>(&mut self, past_len: PastLen, iter: I) -> Arc<[Entity]>
    where
        I: IntoIterator,
        I::Item: Bundle<Effect: NoBundleEffect>,
    {
        rev_spawn_batch_with_caller(self, past_len, iter, MaybeLocation::caller())
    }

    #[track_caller]
    fn rev_spawn_empty(&mut self, past_len: PastLen) -> EntityWorldMut {
        rev_spawn_empty_with_caller(self, past_len, MaybeLocation::caller())
    }

    #[track_caller]
    fn rev_try_despawn_single(
        &mut self,
        past_len: PastLen,
        entity: Entity,
    ) -> Result<(), RevEntityError> {
        rev_try_despawn_single_with_caller_world(self, past_len, entity, MaybeLocation::caller())
    }
}

pub trait RevDeferredWorld {
    fn rev_log_scope(&mut self, past_len: PastLen, entity: Entity);
}

impl RevDeferredWorld for DeferredWorld<'_> {
    #[track_caller]
    fn rev_log_scope(&mut self, past_len: PastLen, entity: Entity) {
        self.resource_mut::<RevDespawnCleaner>().log_spawn(
            entity,
            MaybeLocation::caller(),
            past_len,
        );
    }
}

pub(crate) fn rev_init_resource_with_caller<R: Resource + FromWorld>(
    world: &mut World,
    past_len: PastLen,
    caller: MaybeLocation,
) -> ComponentId {
    if !world.contains_resource::<R>() {
        world.buffer_undo_redo(past_len, ResourceSwap::<R>(None));
    }
    world.init_resource::<R>()
}

pub(crate) fn rev_insert_resource_with_caller<R: Resource>(
    world: &mut World,
    past_len: PastLen,
    resource: R,
    caller: MaybeLocation,
) {
    let swap = ResourceSwap(world.remove_resource::<R>());
    world.insert_resource(resource);
    world.buffer_undo_redo(past_len, swap);
}

pub(crate) fn rev_remove_resource_with_caller<R: Resource, Out>(
    world: &mut World,
    past_len: PastLen,
    c: impl FnOnce(&R) -> Out,
    caller: MaybeLocation,
) -> Option<Out> {
    world.remove_resource::<R>().map(|resource| {
        let out = c(&resource);
        world.buffer_undo_redo(past_len, ResourceSwap(Some(resource)));
        out
    })
}

pub(crate) fn rev_spawn_with_caller_world<B: Bundle<Effect: NoBundleEffect>>(
    world: &mut World,
    past_len: PastLen,
    bundle: B,
    caller: MaybeLocation,
) -> EntityWorldMut {
    let mut entity_mut = world.spawn(bundle);
    rev_spawn_despawn_with_caller::<true>(&mut entity_mut, past_len, caller);
    entity_mut
}

pub(crate) fn rev_spawn_batch_with_caller<I>(
    world: &mut World,
    past_len: PastLen,
    iter: I,
    caller: MaybeLocation,
) -> Arc<[Entity]>
where
    I: IntoIterator,
    I::Item: Bundle<Effect: NoBundleEffect>,
{
    struct SpawnBatch {
        entities: Arc<[Entity]>,
        buffers: Option<Box<[Entity]>>,
        caller: MaybeLocation,
    }

    fn move_all_cloner(world: &mut World) -> EntityCloner {
        let mut builder = EntityCloner::build_opt_out(world);
        builder.move_components(true).deny::<RevDespawned>();
        builder.finish()
    }

    impl UndoRedo for SpawnBatch {
        fn undo(&mut self, world: &mut World) {
            let entities = self.entities.iter().copied();
            let buffers = self
                .buffers
                .get_or_insert_with(|| {
                    let buffers = world
                        .spawn_batch(core::iter::repeat_n(RevDespawned, self.entities.len()))
                        .collect::<Box<[_]>>();
                    world
                        .resource_mut::<RevDespawnCleaner>()
                        .log_spawn_buffer_batch(&buffers, self.caller);
                    buffers
                })
                .iter()
                .copied();

            let mut cloner = move_all_cloner(world);
            for (entity, buffer) in entities.zip(buffers) {
                cloner.clone_entity(world, entity, buffer);
            }

            world.insert_batch(
                self.entities
                    .iter()
                    .map(|entity| (*entity, RevDespawned))
                    .rev(),
            );
        }
        fn redo(&mut self, world: &mut World) {
            let id = world.register_component::<RevDespawned>();
            for entity in self.entities.iter() {
                world.entity_mut(*entity).remove_by_id(id);
            }

            let entities = self.entities.iter().copied();
            let buffers = self.buffers.as_ref().unwrap().iter().copied();

            let mut cloner = move_all_cloner(world);
            for (entity, buffer) in entities.zip(buffers) {
                cloner.clone_entity(world, buffer, entity);
            }
        }
    }

    let entities: Arc<[Entity]> = world.spawn_batch(iter).collect();

    world.buffer_undo_redo(
        past_len,
        SpawnBatch {
            entities: entities.clone(),
            buffers: None,
            caller,
        },
    );

    let mut cleaner = world.resource_mut::<RevDespawnCleaner>();
    cleaner.log_spawn_batch(&entities, caller, past_len);
    cleaner.log_spawn_buffer_batch_reserve(entities.len(), caller, past_len);

    entities
}

pub(crate) fn rev_spawn_empty_with_caller(
    world: &mut World,
    past_len: PastLen,
    caller: MaybeLocation,
) -> EntityWorldMut {
    let mut entity_mut = world.spawn_empty();
    rev_spawn_empty_inner(&mut entity_mut, past_len, caller);
    entity_mut
}

pub(crate) fn rev_try_despawn_single_with_caller_world(
    world: &mut World,
    past_len: PastLen,
    entity: Entity,
    caller: MaybeLocation,
) -> Result<(), RevEntityError> {
    match world.get_entity_mut(entity) {
        Ok(entity_mut) => {
            super::entity_world::rev_try_despawn_single_with_caller(entity_mut, past_len, caller)?
        }
        Err(EntityMutableFetchError::EntityDoesNotExist(err)) => Err(err)?,
        Err(EntityMutableFetchError::AliasedMutability(_)) => unreachable!(), // fetching only a single entity
    }
    Ok(())
}

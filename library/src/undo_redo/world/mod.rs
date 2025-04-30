use std::{hash::Hash, sync::Arc};

use bevy::{
    ecs::{
        bundle::{Bundle, BundleId, NoBundleEffect},
        component::ComponentId,
        entity::Entity,
        resource::Resource,
        world::{EntityWorldMut, FromWorld, World},
    },
    log::warn,
};

use crate::meta::RevMeta;

use super::*;

#[cfg(test)]
mod test;

pub trait RevWorld {
    // buffer methods

    fn buffer_components_in_progress(&self) -> Option<BufferInProgress>;

    fn buffer_components(
        &mut self,
        entity: Entity,
        at: BufferAt,
        components: &[ComponentId],
    ) -> Result<Option<Entity>, RevEntityError>;

    fn buffer_components_cached<T: AsRef<[ComponentId]>>(
        &mut self,
        entity: Entity,
        key: impl Hash + 'static,
        components: impl FnOnce(&mut World) -> (BufferAt, T),
    ) -> Result<Option<Entity>, RevEntityError>;

    fn buffer_bundle(
        &mut self,
        entity: Entity,
        at: BufferAt,
        bundle: BundleId,
    ) -> Result<Option<Entity>, RevEntityError>;

    // additional fallible methods

    fn rev_try_spawn<B: Bundle>(&mut self, bundle: B)
    -> Result<EntityWorldMut, RevMetaNotLogError>;

    fn rev_try_spawn_empty(&mut self) -> Result<EntityWorldMut, RevMetaNotLogError>;

    fn rev_try_spawn_batch<I>(&mut self, iter: I) -> Result<Arc<[Entity]>, RevMetaNotLogError>
    where
        I: IntoIterator,
        I::Item: Bundle<Effect: NoBundleEffect>;

    // the methods here are purposely sorted alphabetically to make it easily comparable to bevy's docs
    // unmentioned methods are either
    // a) unrelated to reversible structural changes OR
    // b) deprecated in bevy OR
    // c) missed by accident!

    /// Reversible version of [`World::despawn`].
    #[track_caller]
    fn rev_despawn(&mut self, entity: Entity) -> bool {
        self.rev_try_despawn(entity)
            .inspect_err(|err| warn!("entity {entity} could not be reversibly despawned: {err}"))
            .is_ok()
    }

    /// Reversible version of [`World::get_resource_or_init`].
    fn rev_get_resource_or_init<R: Resource + FromWorld>(&mut self) -> Mut<'_, R>;

    /// Reversible version of [`World::get_resource_or_insert_with`].
    fn rev_get_resource_or_insert_with<R: Resource>(
        &mut self,
        func: impl FnOnce() -> R,
    ) -> Mut<'_, R>;

    // rev_init_non_send_resource
    // out of scope due Send bound on UndoRedo

    /// Reversible version of [`World::init_resource`].
    fn rev_init_resource<R: Resource + FromWorld>(&mut self);

    /// Reversible version of [`World::insert_batch`].
    #[track_caller]
    fn rev_insert_batch<I, B>(&mut self, batch: I)
    where
        I: IntoIterator,
        I::IntoIter: Iterator<Item = (Entity, B)>,
        B: Bundle<Effect: NoBundleEffect>,
    {
        self.rev_try_insert_batch(batch)
            .unwrap_or_else(|err| panic!("{err}"))
    }

    /// Reversible version of [`World::insert_batch_if_new`].
    #[track_caller]
    fn rev_insert_batch_if_new<I, B>(&mut self, batch: I)
    where
        I: IntoIterator,
        I::IntoIter: Iterator<Item = (Entity, B)>,
        B: Bundle<Effect: NoBundleEffect>,
    {
        self.rev_try_insert_batch_if_new(batch)
            .unwrap_or_else(|err| panic!("{err}"))
    }

    // rev_insert_non_send_by_id
    // out of scope due Send bound on UndoRedo

    // rev_insert_non_send_resource
    // out of scope due Send bound on UndoRedo

    /// Reversible version of [`World::insert_resource`].
    fn rev_insert_resource<R: Resource>(&mut self, resource: R);

    // rev_insert_resource_by_id
    // blocked on https://github.com/bevyengine/bevy/pull/17485

    // rev_remove_non_send_by_id
    // out of scope due Send bound on UndoRedo

    /// Reversible version of [`World::remove_resource`].
    fn rev_remove_resource<R: Resource, Out>(&mut self, c: impl FnOnce(&R) -> Out) -> Option<Out>;

    /// rev_remove_resource_by_id
    // blocked on https://github.com/bevyengine/bevy/pull/17485

    /// Reversible version of [`World::spawn`].
    #[track_caller]
    fn rev_spawn<B: Bundle>(&mut self, bundle: B) -> EntityWorldMut {
        self.rev_try_spawn(bundle)
            .unwrap_or_else(|err| panic!("{err}"))
    }

    /// Reversible version of [`World::spawn_batch`].
    #[track_caller]
    fn rev_spawn_batch<I>(&mut self, iter: I) -> Arc<[Entity]>
    where
        I: IntoIterator,
        I::Item: Bundle<Effect: NoBundleEffect>,
    {
        self.rev_try_spawn_batch(iter)
            .unwrap_or_else(|err| panic!("{err}"))
    }

    /// Reversible version of [`World::spawn_empty`].
    #[track_caller]
    fn rev_spawn_empty(&mut self) -> EntityWorldMut<'_> {
        self.rev_try_spawn_empty()
            .unwrap_or_else(|err| panic!("{err}"))
    }

    /// Reversible version of [`World::try_despawn`].
    fn rev_try_despawn(&mut self, entity: Entity) -> Result<(), RevEntityError>;

    /// Reversible version of [`World::try_insert_batch`].
    fn rev_try_insert_batch<I, B>(&mut self, batch: I) -> Result<(), RevEntitiesError>
    where
        I: IntoIterator,
        I::IntoIter: Iterator<Item = (Entity, B)>,
        B: Bundle<Effect: NoBundleEffect>;

    /// Reversible version of [`World::try_insert_batch_if_new`].
    fn rev_try_insert_batch_if_new<I, B>(&mut self, batch: I) -> Result<(), RevEntitiesError>
    where
        I: IntoIterator,
        I::IntoIter: Iterator<Item = (Entity, B)>,
        B: Bundle<Effect: NoBundleEffect>;
}

impl RevWorld for World {
    fn buffer_components_in_progress(&self) -> Option<BufferInProgress> {
        buffer_components_in_progress(self)
    }

    fn buffer_components(
        &mut self,
        entity: Entity,
        at: BufferAt,
        components: &[ComponentId],
    ) -> Result<Option<Entity>, RevEntityError> {
        let bundle = components_to_bundle(self, components);
        self.buffer_bundle(entity, at, bundle)
    }

    fn buffer_components_cached<T: AsRef<[ComponentId]>>(
        &mut self,
        entity: Entity,
        key: impl Hash + 'static,
        components: impl FnOnce(&mut World) -> (BufferAt, T),
    ) -> Result<Option<Entity>, RevEntityError> {
        let marker = DespawnAtOutOfLog::for_buffer(self.get_resource::<RevMeta>())?;
        buffer_components_cached(self, entity, key, components, marker)
    }

    fn buffer_bundle(
        &mut self,
        entity: Entity,
        at: BufferAt,
        bundle: BundleId,
    ) -> Result<Option<Entity>, RevEntityError> {
        let marker = DespawnAtOutOfLog::for_buffer(self.get_resource::<RevMeta>())?;
        buffer_bundle(self, entity, at, bundle, marker)
    }

    #[track_caller]
    fn rev_try_spawn<B: Bundle>(
        &mut self,
        bundle: B,
    ) -> Result<EntityWorldMut, RevMetaNotLogError> {
        let marker = DespawnAtOutOfLog::for_spawn_despawn(self.get_resource::<RevMeta>())?;
        let mut entity_mut = self.spawn(bundle);
        let entity = entity_mut.id();
        entity_mut.buffer_undo_redo(Spawn { entity, marker });
        Ok(entity_mut)
    }

    #[track_caller]
    fn rev_try_spawn_batch<I>(&mut self, iter: I) -> Result<Arc<[Entity]>, RevMetaNotLogError>
    where
        I: IntoIterator,
        I::Item: Bundle<Effect: NoBundleEffect>,
    {
        struct SpawnBatch {
            entities: Arc<[Entity]>,
            marker: DespawnAtOutOfLog,
        }

        impl UndoRedo for SpawnBatch {
            fn undo(&mut self, world: &mut World) {
                world.insert_batch(self.entities.iter().map(|entity| (*entity, self.marker)));
            }
            fn redo(&mut self, world: &mut World) {
                let id = world.component_id::<DespawnAtOutOfLog>().expect("todo");
                for entity in self.entities.iter() {
                    world.entity_mut(*entity).remove_by_id(id);
                }
            }
        }

        let marker = DespawnAtOutOfLog::for_spawn_despawn(self.get_resource::<RevMeta>())?;
        let entities: Arc<[Entity]> = self.spawn_batch(iter).collect();
        self.buffer_undo_redo(SpawnBatch {
            entities: entities.clone(),
            marker,
        });
        Ok(entities)
    }

    #[track_caller]
    fn rev_try_spawn_empty(&mut self) -> Result<EntityWorldMut, RevMetaNotLogError> {
        let marker = DespawnAtOutOfLog::for_spawn_despawn(self.get_resource::<RevMeta>())?;
        let mut entity_mut = self.spawn_empty();
        let entity = entity_mut.id();
        entity_mut.buffer_undo_redo(Spawn { entity, marker });
        Ok(entity_mut)
    }

    fn rev_init_resource<R: Resource + FromWorld>(&mut self) {
        if !self.contains_resource::<R>() {
            self.init_resource::<R>();
            self.buffer_undo_redo(ResourceSwap::<R>(None));
        }
    }

    fn rev_insert_resource<R: Resource>(&mut self, resource: R) {
        let swap = ResourceSwap(self.remove_resource::<R>());
        self.insert_resource(resource);
        self.buffer_undo_redo(swap);
    }

    fn rev_remove_resource<R: Resource, Out>(&mut self, c: impl FnOnce(&R) -> Out) -> Option<Out> {
        self.remove_resource::<R>().map(|resource| {
            let out = c(&resource);
            self.buffer_undo_redo(ResourceSwap(Some(resource)));
            out
        })
    }

    #[track_caller]
    fn rev_try_despawn(&mut self, entity: Entity) -> Result<(), RevEntityError> {
        rev_despawn_inner(self.get_entity_mut(entity).map_err(|err| match err {
            EntityMutableFetchError::EntityDoesNotExist(err) => err,
            EntityMutableFetchError::AliasedMutability(_) => unreachable!(),
        })?)
    }

    #[track_caller]
    fn rev_try_insert_batch<I, B>(&mut self, batch: I) -> Result<(), RevEntitiesError>
    where
        I: IntoIterator,
        I::IntoIter: Iterator<Item = (Entity, B)>,
        B: Bundle<Effect: NoBundleEffect>,
    {
        try_insert_batch_inner(self, batch, InsertMode::Replace)
    }

    #[track_caller]
    fn rev_try_insert_batch_if_new<I, B>(&mut self, batch: I) -> Result<(), RevEntitiesError>
    where
        I: IntoIterator,
        I::IntoIter: Iterator<Item = (Entity, B)>,
        B: Bundle<Effect: NoBundleEffect>,
    {
        try_insert_batch_inner(self, batch, InsertMode::Keep)
    }

    fn rev_get_resource_or_init<R: Resource + FromWorld>(&mut self) -> Mut<'_, R> {
        self.rev_init_resource::<R>();
        self.resource_mut::<R>()
    }

    fn rev_get_resource_or_insert_with<R: Resource>(
        &mut self,
        func: impl FnOnce() -> R,
    ) -> Mut<'_, R> {
        if !self.contains_resource::<R>() {
            self.buffer_undo_redo(ResourceSwap::<R>(None));
        }
        self.get_resource_or_insert_with(func)
    }
}

#[track_caller]
fn try_insert_batch_inner<I, B>(
    world: &mut World,
    batch: I,
    insert_mode: InsertMode,
) -> Result<(), RevEntitiesError>
where
    I: IntoIterator,
    I::IntoIter: Iterator<Item = (Entity, B)>,
    B: Bundle<Effect: NoBundleEffect>,
{
    let marker = DespawnAtOutOfLog::for_buffer(world.get_resource::<RevMeta>())?;
    let mut invalid = Vec::new();
    let mut rev_despawned = Vec::new();
    let batch: Vec<_> = batch
        .into_iter()
        .filter(|&(entity, _)| {
            world
                .get_entity(entity)
                .map_err(|err| invalid.push(err))
                .map(|entity| entity.location().archetype_id)
                .is_ok_and(|archetype_id| pre_insert::<B>(world, entity, archetype_id, insert_mode, marker)
                    .map_err(|err| match err {
                        RevEntityError::EntityRevDespawnedError(err) => rev_despawned.push(err),
                        _ => unreachable!("EntityDoesNotExistError collected earlier, no other errors returned by pre_insert"),
                    })
                    .is_ok())
        })
        .collect();
    match insert_mode {
        InsertMode::Replace => world.insert_batch(batch),
        InsertMode::Keep => world.insert_batch_if_new(batch),
    }

    if invalid.is_empty() && rev_despawned.is_empty() {
        Ok(())
    } else {
        Err(RevEntitiesError::BadEntities {
            invalid,
            rev_despawned,
        })
    }
}

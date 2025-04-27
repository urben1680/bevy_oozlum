use std::{
    any::TypeId,
    hash::{BuildHasher, Hash, Hasher},
    sync::Arc,
};

use bevy::{
    ecs::{
        bundle::{Bundle, BundleId, NoBundleEffect},
        change_detection::MaybeLocation,
        component::ComponentId,
        entity::{Entity, EntityLocation},
        resource::Resource,
        world::{EntityWorldMut, FromWorld, World, error::TryInsertBatchError},
    },
    log::warn,
    platform::{
        collections::HashMap,
        hash::{FixedHasher, PassHash},
    },
};

use crate::meta::RevMeta;

use super::*;

#[cfg(test)]
mod test;

pub trait RevWorld {
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

    // the methods here are purposely sorted alphabetically to make it easily comparable to bevy's docs
    // unmentioned methods are either
    // a) unrelated to reversible structural changes OR
    // b) deprecated in bevy OR
    // c) missed by accident!

    /// Reversible version of [`World::despawn`].
    fn rev_despawn(&mut self, entity: Entity) -> bool;

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
    fn rev_insert_batch<I, B>(&mut self, batch: I)
    where
        I: IntoIterator,
        I::IntoIter: Iterator<Item = (Entity, B)>,
        B: Bundle<Effect: NoBundleEffect>;

    /// Reversible version of [`World::insert_batch_if_new`].
    fn rev_insert_batch_if_new<I, B>(&mut self, batch: I)
    where
        I: IntoIterator,
        I::IntoIter: Iterator<Item = (Entity, B)>,
        B: Bundle<Effect: NoBundleEffect>;

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
    fn rev_spawn<B: Bundle>(&mut self, bundle: B) -> EntityWorldMut;

    /// Reversible version of [`World::spawn_batch`].
    fn rev_spawn_batch<I>(&mut self, iter: I) -> Arc<[Entity]>
    where
        I: IntoIterator,
        I::Item: Bundle<Effect: NoBundleEffect>;

    /// Reversible version of [`World::spawn_empty`].
    fn rev_spawn_empty(&mut self) -> EntityWorldMut<'_>;

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

    // additional fallible methods

    fn rev_try_spawn<B: Bundle>(
        &mut self,
        bundle: B,
    ) -> Result<EntityWorldMut, RevSpawnError>;

    fn rev_try_spawn_empty(&mut self) -> Result<EntityWorldMut, RevSpawnError>;

    fn rev_try_spawn_batch<I>(
        &mut self,
        iter: I,
    ) -> Result<Arc<[Entity]>, RevSpawnError>
    where
        I: IntoIterator,
        I::Item: Bundle<Effect: NoBundleEffect>;
}

impl RevWorld for World {
    fn buffer_components_in_progress(&self) -> Option<BufferInProgress> {
        buffer_components_in_progress(self)
    }

    fn rev_despawn(&mut self, entity: Entity) -> bool {
        self.rev_try_despawn(entity)
            .inspect_err(|error| warn!("{}", BufferBundleError {
                entity,

            }))
            .is_ok()
    }

    fn rev_insert_batch<I, B>(&mut self, batch: I)
    where
        I: IntoIterator,
        I::IntoIter: Iterator<Item = (Entity, B)>,
        B: Bundle<Effect: NoBundleEffect>,
    {
        let batch: Vec<_> = batch.into_iter().collect();
        for &(entity, _) in batch.iter() {
            let archetype_id = self.entities().get(entity).unwrap().archetype_id;
            pre_insert::<B>(self, entity, archetype_id, InsertMode::Replace).expect("todo");
        }
        self.insert_batch(batch);
    }

    fn rev_insert_batch_if_new<I, B>(&mut self, batch: I)
    where
        I: IntoIterator,
        I::IntoIter: Iterator<Item = (Entity, B)>,
        B: Bundle<Effect: NoBundleEffect>,
    {
        let batch = batch.into_iter();
        let (min, max) = batch.size_hint();
        let mut entities = Vec::with_capacity(max.unwrap_or(min));
        self.insert_batch_if_new(batch.inspect(|(entity, _)| entities.push(*entity)));
        for entity in entities {
            let archetype_id = self.entities().get(entity).unwrap().archetype_id;
            pre_insert::<B>(self, entity, archetype_id, InsertMode::Keep).expect("todo");
        }
    }

    fn rev_try_spawn<B: Bundle>(
        &mut self,
        bundle: B,
    ) -> Result<EntityWorldMut, RevSpawnError> {
        let marker = DespawnAtOutOfLog::new(self.get_resource::<RevMeta>())?;
        let mut entity_mut = self.spawn(bundle);
        let entity = entity_mut.id();
        entity_mut.buffer_undo_redo(Spawn { entity, marker });
        Ok(entity_mut)
    }

    fn rev_spawn<B: Bundle>(&mut self, bundle: B) -> EntityWorldMut {
        self.rev_try_spawn(bundle).expect("todo")
    }

    fn rev_try_spawn_batch<I>(
        &mut self,
        iter: I,
    ) -> Result<Arc<[Entity]>, RevSpawnError>
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

        match DespawnAtOutOfLog::new(self.get_resource::<RevMeta>()) {
            Ok(marker) => {
                let entities: Arc<[Entity]> = self.spawn_batch(iter).collect();
                self.buffer_undo_redo(SpawnBatch {
                    entities: entities.clone(),
                    marker,
                });
                Ok(entities)
            }
            Err(err) => Err((iter, err)),
        }
    }

    fn rev_spawn_batch<I>(&mut self, iter: I) -> Arc<[Entity]>
    where
        I: IntoIterator,
        I::Item: Bundle<Effect: NoBundleEffect>,
    {
        self.rev_try_spawn_batch(iter)
            .map_err(|(_, err)| err)
            .expect("todo")
    }

    fn rev_try_spawn_empty(&mut self) -> Result<EntityWorldMut, RevSpawnError> {
        let marker = DespawnAtOutOfLog::new(self.get_resource::<RevMeta>())?;
        let mut entity_mut = self.spawn_empty();
        let entity = entity_mut.id();
        entity_mut.buffer_undo_redo(Spawn { entity, marker });
        Ok(entity_mut)
    }

    fn rev_spawn_empty(&mut self) -> EntityWorldMut<'_> {
        self.rev_try_spawn_empty().expect("todo")
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
        #[derive(Resource, Default)]
        pub(crate) struct CachedBundles(HashMap<u64, (BufferAt, BundleId), PassHash>);
        fn type_id_of_var<T: 'static>(_: &T) -> TypeId {
            TypeId::of::<T>()
        }

        let mut hasher = FixedHasher::default().build_hasher();
        type_id_of_var(&key).hash(&mut hasher);
        key.hash(&mut hasher);
        let key = hasher.finish();

        let mut cache = self.remove_resource::<CachedBundles>().unwrap_or_default();
        let (at, bundle) = *cache.0.entry(key).or_insert_with(|| {
            let (at, components) = components(self);
            let components = components.as_ref();
            (at, components_to_bundle(self, &components))
        });
        self.insert_resource(cache);
        self.buffer_bundle(entity, at, bundle)
    }

    fn buffer_bundle(
        &mut self,
        entity: Entity,
        at: BufferAt,
        bundle: BundleId,
    ) -> Result<Option<Entity>, RevEntityError> {
        let location = MaybeLocation::caller();
        match DespawnAtOutOfLog::new(self.get_resource::<RevMeta>()) {
            Ok(marker) => buffer_bundle(self, entity, at, bundle, marker, location),
            Err(err) => Err(BufferBundleError {
                entity,
                at,
                bundle,
                variant: err.into(),
                location,
            }),
        }
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

    #[track_caller]
    fn rev_try_despawn(&mut self, entity: Entity) -> Result<(), RevEntityError> {
        let entity_mut = self.get_entity_mut(entity)?;
        rev_despawn_inner(entity_mut)
    }

    fn rev_try_insert_batch<I, B>(&mut self, batch: I) -> Result<(), RevEntitiesError>
    where
        I: IntoIterator,
        I::IntoIter: Iterator<Item = (Entity, B)>,
        B: Bundle<Effect: NoBundleEffect>,
    {
        try_insert_batch_inner(self, batch, InsertMode::Replace)
    }

    fn rev_try_insert_batch_if_new<I, B>(&mut self, batch: I) -> Result<(), RevEntitiesError>
    where
        I: IntoIterator,
        I::IntoIter: Iterator<Item = (Entity, B)>,
        B: Bundle<Effect: NoBundleEffect>,
    {
        try_insert_batch_inner(self, batch, InsertMode::Keep)
    }
}

fn try_insert_batch_inner<I, B>(
    world: &mut World,
    batch: I,
    insert_mode: InsertMode,
) -> Result<(), TryInsertBatchError>
where
    I: IntoIterator,
    I::IntoIter: Iterator<Item = (Entity, B)>,
    B: Bundle<Effect: NoBundleEffect>,
{
    let mut invalid_entities = Vec::new();
    let batch: Vec<_> = batch
        .into_iter()
        .filter(|&(entity, _)| {
            world
                .entities()
                .get(entity)
                .filter(|&EntityLocation { archetype_id, .. }| {
                    pre_insert::<B>(world, entity, archetype_id, insert_mode).is_ok()
                })
                .or_else(|| {
                    invalid_entities.push(entity);
                    None
                })
                .is_some()
        })
        .collect();
    match insert_mode {
        InsertMode::Replace => world.insert_batch(batch),
        InsertMode::Keep => world.insert_batch_if_new(batch),
    }

    if invalid_entities.is_empty() {
        Ok(())
    } else {
        Err(TryInsertBatchError {
            bundle_type: core::any::type_name::<B>(),
            entities: invalid_entities,
        })
    }
}

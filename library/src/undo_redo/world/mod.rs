use std::{hash::Hash, sync::Arc};

use bevy::{
    ecs::{
        bundle::{Bundle, BundleId, NoBundleEffect},
        component::ComponentId,
        entity::Entity,
        resource::Resource,
        world::{FromWorld, World},
    },
    log::warn,
};

use crate::meta::NonLogNow;

use super::*;

pub trait RevWorld {
    fn redo_and_buffer(&mut self, now: NonLogNow, undo_redo: impl UndoRedo);

    fn buffer_components(
        &mut self,
        now: NonLogNow,
        entity: Entity,
        at: BufferAt,
        components: &[ComponentId],
    ) -> Result<Option<Entity>, RevEntityError>;

    fn buffer_components_cached<T: AsRef<[ComponentId]>>(
        &mut self,
        now: NonLogNow,
        entity: Entity,
        key: impl Hash + 'static,
        components: impl FnOnce(&mut World) -> (BufferAt, T),
    ) -> Result<Option<Entity>, RevEntityError>;

    fn buffer_bundle(
        &mut self,
        now: NonLogNow,
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

    /// Reversible version of [`World::insert_batch`].
    fn rev_insert_batch<I, B>(&mut self, now: NonLogNow, batch: I)
    where
        I: IntoIterator,
        I::IntoIter: Iterator<Item = (Entity, B)>,
        B: Bundle<Effect: NoBundleEffect>;

    /// Reversible version of [`World::insert_batch_if_new`].
    fn rev_insert_batch_if_new<I, B>(&mut self, now: NonLogNow, batch: I)
    where
        I: IntoIterator,
        I::IntoIter: Iterator<Item = (Entity, B)>,
        B: Bundle<Effect: NoBundleEffect>;

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

    /// Reversible version of [`World::spawn_batch`].
    fn rev_spawn_batch<I>(&mut self, now: NonLogNow, iter: I) -> Arc<[Entity]>
    where
        I: IntoIterator,
        I::Item: Bundle<Effect: NoBundleEffect>;

    /// Reversible version of [`World::spawn_empty`].
    fn rev_spawn_empty(&mut self, now: NonLogNow) -> EntityWorldMut;

    /// Reversible version of [`World::try_despawn`].
    fn rev_try_despawn(&mut self, now: NonLogNow, entity: Entity) -> Result<(), RevEntityError>;

    /// Reversible version of [`World::try_insert_batch`].
    fn rev_try_insert_batch<I, B>(
        &mut self,
        now: NonLogNow,
        batch: I,
    ) -> Result<(), RevEntitiesError>
    where
        I: IntoIterator,
        I::IntoIter: Iterator<Item = (Entity, B)>,
        B: Bundle<Effect: NoBundleEffect>;

    /// Reversible version of [`World::try_insert_batch_if_new`].
    fn rev_try_insert_batch_if_new<I, B>(
        &mut self,
        now: NonLogNow,
        batch: I,
    ) -> Result<(), RevEntitiesError>
    where
        I: IntoIterator,
        I::IntoIter: Iterator<Item = (Entity, B)>,
        B: Bundle<Effect: NoBundleEffect>;
}

impl RevWorld for World {
    fn redo_and_buffer(&mut self, now: NonLogNow, mut undo_redo: impl UndoRedo) {
        undo_redo.redo(self);
        self.buffer_undo_redo(now, undo_redo)
    }

    fn buffer_components(
        &mut self,
        now: NonLogNow,
        entity: Entity,
        at: BufferAt,
        components: &[ComponentId],
    ) -> Result<Option<Entity>, RevEntityError> {
        let bundle = components_to_bundle(self, components);
        self.buffer_bundle(now, entity, at, bundle)
    }

    fn buffer_components_cached<T: AsRef<[ComponentId]>>(
        &mut self,
        now: NonLogNow,
        entity: Entity,
        key: impl Hash + 'static,
        components: impl FnOnce(&mut World) -> (BufferAt, T),
    ) -> Result<Option<Entity>, RevEntityError> {
        let marker = DisabledToDespawn::for_buffer(now.0);
        buffer_components_cached(self, now, entity, key, components, marker)
    }

    fn buffer_bundle(
        &mut self,
        now: NonLogNow,
        entity: Entity,
        at: BufferAt,
        bundle: BundleId,
    ) -> Result<Option<Entity>, RevEntityError> {
        let marker = DisabledToDespawn::for_buffer(now.0);
        buffer_bundle(self, now, entity, at, bundle, marker)
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

    #[track_caller]
    fn rev_insert_batch<I, B>(&mut self, now: NonLogNow, batch: I)
    where
        I: IntoIterator,
        I::IntoIter: Iterator<Item = (Entity, B)>,
        B: Bundle<Effect: NoBundleEffect>,
    {
        self.rev_try_insert_batch(now, batch)
            .unwrap_or_else(|err| panic!("{err}"))
    }

    #[track_caller]
    fn rev_insert_batch_if_new<I, B>(&mut self, now: NonLogNow, batch: I)
    where
        I: IntoIterator,
        I::IntoIter: Iterator<Item = (Entity, B)>,
        B: Bundle<Effect: NoBundleEffect>,
    {
        self.rev_try_insert_batch_if_new(now, batch)
            .unwrap_or_else(|err| panic!("{err}"))
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
    fn rev_spawn_batch<I>(&mut self, now: NonLogNow, iter: I) -> Arc<[Entity]>
    where
        I: IntoIterator,
        I::Item: Bundle<Effect: NoBundleEffect>,
    {
        let marker = DisabledToDespawn::for_spawn_despawn(now.0);
        let entities: Arc<[Entity]> = self.spawn_batch(iter).collect();
        self.buffer_undo_redo(
            now,
            Spawn::<Arc<[Entity]>> {
                spawned: entities.clone(),
                marker,
            },
        );
        entities
    }

    #[track_caller]
    fn rev_spawn_empty(&mut self, now: NonLogNow) -> EntityWorldMut {
        let mut entity_world_mut = self.spawn_empty();
        let entity = entity_world_mut.id();
        let marker = DisabledToDespawn::for_spawn_despawn(now.0);
        entity_world_mut.buffer_undo_redo(
            now,
            Spawn {
                spawned: [entity],
                marker,
            },
        );
        entity_world_mut
    }

    #[track_caller]
    fn rev_try_despawn(&mut self, now: NonLogNow, entity: Entity) -> Result<(), RevEntityError> {
        match self.get_entity_mut(entity) {
            Ok(entity) => rev_despawn_inner(entity, now),
            Err(EntityMutableFetchError::EntityDoesNotExist(err)) => Err(err.into()),
            Err(EntityMutableFetchError::AliasedMutability(_)) => {
                unreachable!("only one entity accessed")
            }
        }
    }

    #[track_caller]
    fn rev_try_insert_batch<I, B>(
        &mut self,
        now: NonLogNow,
        batch: I,
    ) -> Result<(), RevEntitiesError>
    where
        I: IntoIterator,
        I::IntoIter: Iterator<Item = (Entity, B)>,
        B: Bundle<Effect: NoBundleEffect>,
    {
        try_insert_batch_inner(self, now, batch, InsertMode::Replace)
    }

    #[track_caller]
    fn rev_try_insert_batch_if_new<I, B>(
        &mut self,
        now: NonLogNow,
        batch: I,
    ) -> Result<(), RevEntitiesError>
    where
        I: IntoIterator,
        I::IntoIter: Iterator<Item = (Entity, B)>,
        B: Bundle<Effect: NoBundleEffect>,
    {
        try_insert_batch_inner(self, now, batch, InsertMode::Keep)
    }
}

#[track_caller]
fn try_insert_batch_inner<I, B>(
    world: &mut World,
    now: NonLogNow,
    batch: I,
    insert_mode: InsertMode,
) -> Result<(), RevEntitiesError>
where
    I: IntoIterator,
    I::IntoIter: Iterator<Item = (Entity, B)>,
    B: Bundle<Effect: NoBundleEffect>,
{
    let marker = DisabledToDespawn::for_buffer(now.0);
    let mut error = RevEntitiesError {
        invalid: Vec::new(),
        rev_despawned: Vec::new(),
        rev_despawned_buffers: MaybeLocation::new_with(|| Vec::new()),
    };
    let batch: Vec<_> = batch
        .into_iter()
        .filter(|&(entity, _)| {
            world
                .get_entity(entity)
                .map_err(|err| error.push(err))
                .map(|entity| entity.location().archetype_id)
                .is_ok_and(|archetype_id| {
                    buffer_pre_insert::<B>(world, now, entity, archetype_id, insert_mode, marker)
                        .map_err(|err| error.push(err))
                        .is_ok()
                })
        })
        .collect();
    match insert_mode {
        InsertMode::Replace => world.insert_batch(batch),
        InsertMode::Keep => world.insert_batch_if_new(batch),
    }

    if error.is_empty() { Ok(()) } else { Err(error) }
}
